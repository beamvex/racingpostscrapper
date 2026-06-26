resource "aws_cloudwatch_log_group" "backfill_jun_2023_to_jun_2024_no_processor" {
  name              = "/ecs/racingpost-backfill-jun-2023-to-jun-2024-no-processor"
  retention_in_days = 14
}

resource "aws_ecs_task_definition" "backfill_jun_2023_to_jun_2024_no_processor" {
  family                   = "racingpost-backfill-jun-2023-to-jun-2024-no-processor"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"

  execution_role_arn = aws_iam_role.ecs_task_execution.arn
  task_role_arn      = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name      = "backfill-jun-2023-to-jun-2024-no-processor"
      image     = "512752756525.dkr.ecr.eu-west-2.amazonaws.com/racingpost-scrapper:latest"
      essential = true

      entryPoint = ["/app/backfill_jun_2023_to_jun_2024_no_processor.sh"]

      environment = [
        {
          name  = "SCRAPER_DATA_BUCKET_NAME"
          value = aws_s3_bucket.scraper_data.bucket
        }
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.backfill_jun_2023_to_jun_2024_no_processor.name
          awslogs-region        = "eu-west-2"
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

output "ecs_backfill_jun_2023_to_jun_2024_no_processor_task_definition_arn" {
  value = aws_ecs_task_definition.backfill_jun_2023_to_jun_2024_no_processor.arn
}
