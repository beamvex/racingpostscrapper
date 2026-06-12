resource "aws_ecs_cluster" "main" {
  name = "racingpost-scraper"

  setting {
    name  = "containerInsights"
    value = "enabled"
  }
}
