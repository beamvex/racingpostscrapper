#!/bin/bash
DOCKER_BUILDKIT=1 docker build --platform linux/amd64 --provenance=false -t racingpost-scrapper ..
