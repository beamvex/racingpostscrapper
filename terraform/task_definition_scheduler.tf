resource "aws_cloudwatch_log_group" "scheduler" {
  name              = "/ecs/racingpost-scheduler"
  retention_in_days = 14
}

resource "aws_ecs_task_definition" "scheduler" {
  family                   = "racingpost-scheduler"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"

  execution_role_arn = aws_iam_role.ecs_task_execution.arn
  task_role_arn      = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name      = "scheduler"
      image     = "512752756525.dkr.ecr.eu-west-2.amazonaws.com/racingpost-scrapper:latest"
      essential = true

      command = [
        "python3",
        "/app/schedule_today.py"
      ]

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
          name  = "ECS_CLUSTER_ARN"
          value = aws_ecs_cluster.main.arn
        },
        {
          name  = "EVENTBRIDGE_ROLE_ARN"
          value = aws_iam_role.eventbridge_ecs_run_task.arn
        },
        {
          name  = "ECS_SUBNETS"
          value = "${aws_subnet.public_a.id},${aws_subnet.public_b.id}"
        },
        {
          name  = "ECS_SECURITY_GROUPS"
          value = aws_security_group.ecs_tasks.id
        },
        {
          name  = "ECS_TASKDEF_RACECARD_ARN"
          value = aws_ecs_task_definition.racecard_scraper.arn
        },
        {
          name  = "ECS_TASKDEF_RESULTS_ARN"
          value = aws_ecs_task_definition.scraper.arn
        },
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.scheduler.name
          awslogs-region        = "eu-west-2"
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

output "ecs_scheduler_task_definition_arn" {
  value = aws_ecs_task_definition.scheduler.arn
}
