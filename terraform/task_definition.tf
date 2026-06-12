resource "aws_cloudwatch_log_group" "scraper" {
  name              = "/ecs/racingpost-scraper"
  retention_in_days = 14
}

resource "aws_ecs_task_definition" "scraper" {
  family                   = "racingpost-scraper"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "1024"
  memory                   = "2048"

  execution_role_arn = aws_iam_role.ecs_task_execution.arn
  task_role_arn      = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name      = "scraper"
      image     = "512752756525.dkr.ecr.eu-west-2.amazonaws.com/racingpost-scrapper:latest"
      essential = true

      environment = [
        {
          name  = "SCRAPER_DATA_BUCKET_ARN"
          value = aws_s3_bucket.scraper_data.arn
        }
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.scraper.name
          awslogs-region        = "eu-west-2"
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

output "ecs_task_definition_arn" {
  value = aws_ecs_task_definition.scraper.arn
}
