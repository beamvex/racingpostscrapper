resource "aws_cloudwatch_log_group" "processor_trigger" {
  name              = "/ecs/racingpost-processor-trigger"
  retention_in_days = 14
}

resource "aws_ecs_task_definition" "processor_trigger" {
  family                   = "racingpost-processor-trigger"
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = "256"
  memory                   = "512"

  execution_role_arn = aws_iam_role.ecs_task_execution.arn
  task_role_arn      = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name      = "processor-trigger"
      image     = "512752756525.dkr.ecr.eu-west-2.amazonaws.com/racingpost-scrapper:latest"
      essential = true

      entryPoint = ["/app/trigger_processor_months.sh"]

      environment = [
        {
          name  = "AWS_REGION"
          value = "eu-west-2"
        },
        {
          name  = "ECS_CLUSTER_ARN"
          value = aws_ecs_cluster.main.arn
        },
        {
          name  = "PROCESSOR_TASK_DEFINITION_ARN"
          value = aws_ecs_task_definition.processor.arn
        },
        {
          name  = "ECS_SUBNETS_CSV"
          value = join(",", [aws_subnet.public_a.id, aws_subnet.public_b.id])
        },
        {
          name  = "ECS_SECURITY_GROUP_ID"
          value = aws_security_group.ecs_tasks.id
        },
        {
          name  = "PROCESS_START_MONTH"
          value = "2024-06"
        },
        {
          name  = "PROCESS_END_MONTH"
          value = "2026-06"
        }
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.processor_trigger.name
          awslogs-region        = "eu-west-2"
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

output "ecs_processor_trigger_task_definition_arn" {
  value = aws_ecs_task_definition.processor_trigger.arn
}
