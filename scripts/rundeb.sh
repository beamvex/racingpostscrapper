#!/bin/bash
bash ./builddocker.sh

docker run --rm -it -p 3001:3001 -v ./data:/data racingpost-scrapper:latest /bin/bash