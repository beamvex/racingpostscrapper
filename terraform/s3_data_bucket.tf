resource "aws_s3_bucket" "scraper_data" {
  bucket_prefix = "racingpost-scraper-data-"
}

resource "aws_s3_bucket_public_access_block" "scraper_data" {
  bucket = aws_s3_bucket.scraper_data.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_versioning" "scraper_data" {
  bucket = aws_s3_bucket.scraper_data.id

  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "scraper_data" {
  bucket = aws_s3_bucket.scraper_data.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

output "scraper_data_bucket_arn" {
  value = aws_s3_bucket.scraper_data.arn
}
