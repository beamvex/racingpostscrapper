data "aws_iam_policy_document" "lambda_assume_role" {
  statement {
    effect = "Allow"

    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }

    actions = ["sts:AssumeRole"]
  }
}

resource "aws_iam_role" "lambda" {
  name               = "racingpost-scraper-lambda"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume_role.json
}

resource "aws_iam_role_policy_attachment" "lambda_basic" {
  role       = aws_iam_role.lambda.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

data "aws_iam_policy_document" "lambda_s3" {
  statement {
    effect = "Allow"

    actions = [
      "s3:ListBucket"
    ]

    resources = [
      aws_s3_bucket.scraper_data.arn
    ]
  }

  statement {
    effect = "Allow"

    actions = [
      "s3:GetObject"
    ]

    resources = [
      "${aws_s3_bucket.scraper_data.arn}/*"
    ]
  }
}

resource "aws_iam_role_policy" "lambda_s3" {
  name   = "racingpost-probabilities-s3"
  role   = aws_iam_role.lambda.id
  policy = data.aws_iam_policy_document.lambda_s3.json
}

data "archive_file" "lambda_zip" {
  type        = "zip"
  source_file = "${path.module}/lambda_function.py"
  output_path = "${path.module}/lambda_function.zip"
}

resource "aws_lambda_function" "probabilities" {
  filename         = data.archive_file.lambda_zip.output_path
  function_name    = "racingpost-probabilities"
  role             = aws_iam_role.lambda.arn
  handler          = "lambda_function.lambda_handler"
  runtime          = "python3.11"
  source_code_hash = data.archive_file.lambda_zip.output_base64sha256
  timeout          = 30
  memory_size      = 512

  # AWS SDK for pandas 3.17.0 — eu-west-2 / Python 3.11 / x86_64
  # https://aws-sdk-pandas.readthedocs.io/en/stable/layers.html
  layers = ["arn:aws:lambda:eu-west-2:336392948345:layer:AWSSDKPandas-Python311:33"]

  environment {
    variables = {
      SCRAPER_DATA_BUCKET_NAME = aws_s3_bucket.scraper_data.bucket
    }
  }
}

# --- API Gateway ---

resource "aws_apigatewayv2_api" "probabilities" {
  name          = "racingpost-probabilities"
  protocol_type = "HTTP"
}

resource "aws_apigatewayv2_integration" "probabilities" {
  api_id           = aws_apigatewayv2_api.probabilities.id
  integration_type = "AWS_PROXY"
  integration_uri  = aws_lambda_function.probabilities.arn
}

resource "aws_apigatewayv2_route" "probabilities_root" {
  api_id    = aws_apigatewayv2_api.probabilities.id
  route_key = "GET /"
  target    = "integrations/${aws_apigatewayv2_integration.probabilities.id}"
}

resource "aws_apigatewayv2_route" "probabilities_date" {
  api_id    = aws_apigatewayv2_api.probabilities.id
  route_key = "GET /{date}"
  target    = "integrations/${aws_apigatewayv2_integration.probabilities.id}"
}

resource "aws_apigatewayv2_stage" "probabilities" {
  api_id      = aws_apigatewayv2_api.probabilities.id
  name        = "$default"
  auto_deploy = true
}

resource "aws_lambda_permission" "apigateway" {
  statement_id  = "AllowAPIGatewayInvoke"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.probabilities.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_apigatewayv2_api.probabilities.execution_arn}/*/*"
}

output "probabilities_api_url" {
  value = aws_apigatewayv2_stage.probabilities.invoke_url
}

# --- Starter Lambda for EventBridge Scheduler ---

data "archive_file" "starter_lambda_zip" {
  type        = "zip"
  source_file = "${path.module}/../lambda/starter_lambda.py"
  output_path = "${path.module}/starter_lambda.zip"
}

resource "aws_lambda_function" "starter" {
  filename         = data.archive_file.starter_lambda_zip.output_path
  function_name    = "racingpost-starter"
  role             = aws_iam_role.lambda.arn
  handler          = "starter_lambda.lambda_handler"
  runtime          = "python3.11"
  source_code_hash = data.archive_file.starter_lambda_zip.output_base64sha256
  timeout          = 30
  memory_size      = 256

  environment {
    variables = {
      ECS_CLUSTER_ARN          = aws_ecs_cluster.main.arn
      ECS_TASKDEF_PIPELINE_ARN = aws_ecs_task_definition.daily_pipeline.arn
      ECS_SUBNETS              = join(",", [aws_subnet.public_a.id, aws_subnet.public_b.id])
      ECS_SECURITY_GROUPS      = aws_security_group.ecs_tasks.id
    }
  }
}

# Grant Lambda permission to run ECS tasks
data "aws_iam_policy_document" "starter_lambda_ecs" {
  statement {
    effect = "Allow"
    actions = [
      "ecs:RunTask"
    ]
    resources = [
      aws_ecs_task_definition.daily_pipeline.arn
    ]
  }

  statement {
    effect = "Allow"
    actions = [
      "iam:PassRole"
    ]
    resources = [
      aws_iam_role.ecs_task.arn
    ]
  }
}

resource "aws_iam_role_policy" "starter_lambda_ecs" {
  name   = "racingpost-starter-ecs"
  role   = aws_iam_role.lambda.id
  policy = data.aws_iam_policy_document.starter_lambda_ecs.json
}

# Grant EventBridge Scheduler permission to invoke the starter Lambda
resource "aws_lambda_permission" "scheduler" {
  statement_id  = "AllowSchedulerInvoke"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.starter.function_name
  principal     = "scheduler.amazonaws.com"
}

output "starter_lambda_arn" {
  value = aws_lambda_function.starter.arn
}
