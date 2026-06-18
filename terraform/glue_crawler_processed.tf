resource "aws_glue_crawler" "processed" {
  name          = "racingpost-processed"
  role          = aws_iam_role.glue_crawler.arn
  database_name = aws_glue_catalog_database.racingpost.name

  s3_target {
    path = "s3://${aws_s3_bucket.scraper_data.bucket}/processed/"
  }

  configuration = jsonencode({
    Version = 1.0
    CrawlerOutput = {
      Partitions = {
        AddOrUpdateBehavior = "InheritFromTable"
      }
    }
  })

  schema_change_policy {
    update_behavior = "UPDATE_IN_DATABASE"
    delete_behavior = "DEPRECATE_IN_DATABASE"
  }
}

output "glue_crawler_processed_name" {
  value = aws_glue_crawler.processed.name
}
