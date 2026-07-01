resource "aws_cloudwatch_event_rule" "racecard_daily" {
  name                = "racingpost-racecard-scraper-daily"
  description         = "Run racingpost racecard scraper daily at 09:00 UTC"
  schedule_expression = "cron(0 9 * * ? *)"
}

resource "aws_cloudwatch_event_target" "racecard_daily" {
  rule      = aws_cloudwatch_event_rule.racecard_daily.name
  target_id = "ecs-run-task"
  arn       = aws_ecs_cluster.main.arn
  role_arn  = aws_iam_role.eventbridge_ecs_run_task.arn

  ecs_target {
    task_definition_arn = aws_ecs_task_definition.racecard_scraper.arn
    launch_type         = "FARGATE"
    platform_version    = "LATEST"

    network_configuration {
      subnets          = [aws_subnet.public_a.id, aws_subnet.public_b.id]
      security_groups  = [aws_security_group.ecs_tasks.id]
      assign_public_ip = true
    }
  }
}

output "racecard_daily_schedule_rule_name" {
  value = aws_cloudwatch_event_rule.racecard_daily.name
}
