data "aws_iam_role" "lambda" {
  name = "racingpost-scraper-lambda"
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

  environment {
    variables = {
      SCRAPER_DATA_BUCKET_NAME = aws_s3_bucket.scraper_data.bucket
    }
  }
}

