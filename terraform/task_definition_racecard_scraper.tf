resource "aws_cloudwatch_log_group" "racecard_scraper" {
  name              = "/ecs/racingpost-racecard-scraper"
  retention_in_days = 14
}

resource "aws_ecs_task_definition" "racecard_scraper" {
  family                   = "racingpost-racecard-scraper"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "1024"
  memory                   = "2048"

  execution_role_arn = aws_iam_role.ecs_task_execution.arn
  task_role_arn      = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name      = "racecard-scraper"
      image     = "512752756525.dkr.ecr.eu-west-2.amazonaws.com/racingpost-scrapper:latest"
      essential = true

      entryPoint = ["/app/scrape_racecard.sh"]

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
          name  = "S3_BUCKET_NAME"
          value = aws_s3_bucket.scraper_data.bucket
        },
        {
          name  = "ATHENA_OUTPUT_LOCATION"
          value = "s3://${aws_s3_bucket.scraper_data.bucket}/athena/results/"
        }
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.racecard_scraper.name
          awslogs-region        = "eu-west-2"
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

output "ecs_racecard_scraper_task_definition_arn" {
  value = aws_ecs_task_definition.racecard_scraper.arn
}
