resource "aws_glue_catalog_database" "racingpost" {
  name = "racingpost"
}

resource "aws_glue_catalog_table" "processed_full_results" {
  name          = "processed_full_results"
  database_name = aws_glue_catalog_database.racingpost.name
  table_type    = "EXTERNAL_TABLE"

  parameters = {
    classification = "json"
    EXTERNAL       = "TRUE"
  }

  storage_descriptor {
    location      = "s3://${aws_s3_bucket.scraper_data.bucket}/processed/"
    input_format  = "org.apache.hadoop.mapred.TextInputFormat"
    output_format = "org.apache.hadoop.hive.ql.io.HiveIgnoreKeyTextOutputFormat"

    ser_de_info {
      name                  = "json"
      serialization_library = "org.openx.data.jsonserde.JsonSerDe"
    }

    columns {
      name = "url"
      type = "string"
    }

    columns {
      name = "course"
      type = "string"
    }

    columns {
      name = "title"
      type = "string"
    }

    columns {
      name = "race_id"
      type = "string"
    }

    columns {
      name = "runners"
      type = "array<struct<position:string,horse:string,jockey:string,trainer:string,age:string,weight_st:string,weight_lb:string,or:string,ts:string,rpr:string>>"
    }
  }
}
