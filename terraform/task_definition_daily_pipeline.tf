resource "aws_cloudwatch_log_group" "daily_pipeline" {
  name              = "/ecs/racingpost-daily-pipeline"
  retention_in_days = 14
}

resource "aws_ecs_task_definition" "daily_pipeline" {
  family                   = "racingpost-daily-pipeline"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "2048"
  memory                   = "4096"

  execution_role_arn = aws_iam_role.ecs_task_execution.arn
  task_role_arn      = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name      = "daily-pipeline"
      image     = "512752756525.dkr.ecr.eu-west-2.amazonaws.com/racingpost-scrapper:latest"
      essential = true

      entryPoint = ["/app/daily_pipeline.sh"]

      environment = [
        {
          name  = "AWS_REGION"
          value = "eu-west-2"
        },
        {
          name  = "SCRAPER_DATA_BUCKET_NAME"
          value = aws_s3_bucket.scraper_data.bucket
        },
        {
          name  = "SNS_TOPIC_ARN"
          value = aws_sns_topic.pipeline_notifications.arn
        },
        {
          name  = "PROBABILITIES_API_URL"
          value = aws_apigatewayv2_stage.probabilities.invoke_url
        },
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.daily_pipeline.name
          awslogs-region        = "eu-west-2"
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

output "daily_pipeline_task_definition_arn" {
  value = aws_ecs_task_definition.daily_pipeline.arn
}
