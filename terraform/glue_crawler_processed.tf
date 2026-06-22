resource "aws_glue_crawler" "processed" {
  name          = "racingpost-processed"
  role          = aws_iam_role.glue_crawler.arn
  database_name = aws_glue_catalog_database.racingpost.name

  catalog_target {
    database_name = aws_glue_catalog_database.racingpost.name
    tables        = [aws_glue_catalog_table.processed_full_results.name]
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
    delete_behavior = "LOG"
  }
}

output "glue_crawler_processed_name" {
  value = aws_glue_crawler.processed.name
}
