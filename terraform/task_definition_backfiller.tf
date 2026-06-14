resource "aws_cloudwatch_log_group" "backfiller" {
  name              = "/ecs/racingpost-backfiller"
  retention_in_days = 14
}

resource "aws_ecs_task_definition" "backfiller" {
  family                   = "racingpost-backfiller"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"

  execution_role_arn = aws_iam_role.ecs_task_execution.arn
  task_role_arn      = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name      = "backfiller"
      image     = "512752756525.dkr.ecr.eu-west-2.amazonaws.com/racingpost-scrapper:latest"
      essential = true

      entryPoint = ["/app/backfill_last_2_years.sh"]

      environment = [
        {
          name  = "SCRAPER_DATA_BUCKET_NAME"
          value = aws_s3_bucket.scraper_data.bucket
        }
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.backfiller.name
          awslogs-region        = "eu-west-2"
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

output "ecs_backfiller_task_definition_arn" {
  value = aws_ecs_task_definition.backfiller.arn
}
