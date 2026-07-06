resource "aws_cloudwatch_log_group" "scheduler" {
  name              = "/ecs/racingpost-scheduler"
  retention_in_days = 14
}

resource "aws_ecs_task_definition" "scheduler" {
  family                   = "racingpost-scheduler"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "1024"
  memory                   = "2048"

  execution_role_arn = aws_iam_role.ecs_task_execution.arn
  task_role_arn      = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name      = "scheduler"
      image     = "512752756525.dkr.ecr.eu-west-2.amazonaws.com/racingpost-scrapper:latest"
      essential = true

      entryPoint = ["/app/run_scheduler.sh"]

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
          name  = "ECS_TASKDEF_PIPELINE_ARN"
          value = aws_ecs_task_definition.daily_pipeline.arn
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

resource "aws_cloudwatch_event_rule" "scheduler_daily" {
  name                = "racingpost-scheduler-daily"
  description         = "Run scheduler daily at 09:00 UTC"
  schedule_expression = "cron(0 9 * * ? *)"
}

resource "aws_cloudwatch_event_target" "scheduler_daily" {
  rule      = aws_cloudwatch_event_rule.scheduler_daily.name
  target_id = "ecs-run-task"
  arn       = aws_ecs_cluster.main.arn
  role_arn  = aws_iam_role.eventbridge_ecs_run_task.arn

  ecs_target {
    task_definition_arn = aws_ecs_task_definition.scheduler.arn
    launch_type         = "FARGATE"
    platform_version    = "LATEST"

    network_configuration {
      subnets          = [aws_subnet.public_a.id, aws_subnet.public_b.id]
      security_groups  = [aws_security_group.ecs_tasks.id]
      assign_public_ip = true
    }
  }
}

output "ecs_scheduler_task_definition_arn" {
  value = aws_ecs_task_definition.scheduler.arn
}
