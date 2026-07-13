resource "aws_sns_topic" "pipeline_notifications" {
  name = "racingpost-pipeline-notifications"
}

output "pipeline_notifications_topic_arn" {
  value = aws_sns_topic.pipeline_notifications.arn
}

data "aws_iam_policy_document" "ecs_task_sns" {
  statement {
    effect = "Allow"

    actions = [
      "sns:Publish"
    ]

    resources = [
      aws_sns_topic.pipeline_notifications.arn
    ]
  }
}

resource "aws_iam_role_policy" "ecs_task_sns" {
  name   = "racingpost-scraper-sns"
  role   = aws_iam_role.ecs_task.id
  policy = data.aws_iam_policy_document.ecs_task_sns.json
}
