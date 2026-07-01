resource "aws_lb" "probabilities" {
  name               = "racingpost-probabilities"
  internal           = false
  load_balancer_type = "application"
  security_groups    = [aws_security_group.alb.id]
  subnets            = [aws_subnet.public_a.id, aws_subnet.public_b.id]

  enable_deletion_protection = false
}

resource "aws_lb_target_group" "probabilities" {
  name        = "racingpost-probabilities"
  port        = 80
  protocol    = "HTTP"
  vpc_id      = aws_vpc.main.id
  target_type = "lambda"

  health_check {
    enabled = false
  }
}

resource "aws_lb_listener" "http" {
  load_balancer_arn = aws_lb.probabilities.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.probabilities.arn
  }
}

resource "aws_security_group" "alb" {
  name_prefix = "racingpost-alb-"
  vpc_id      = aws_vpc.main.id

  ingress {
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_lb_target_group_attachment" "probabilities" {
  target_group_arn = aws_lb_target_group.probabilities.arn
  target_id        = aws_lambda_function.probabilities.arn
  depends_on       = [aws_lambda_permission.alb]
}

output "alb_dns_name" {
  value = aws_lb.probabilities.dns_name
}
