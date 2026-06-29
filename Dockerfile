FROM rust:trixie 

RUN apt-get update
RUN apt-get install -y net-tools inetutils-tools inetutils-ping nano unzip gpg wget xvfb chromium
RUN apt-get install -y curl
RUN apt-get install -y python3

RUN mv /usr/bin/chromium /usr/bin/og-chromium \
    && ln -s /usr/bin/og-chromium /usr/bin/chromium

RUN curl "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o "awscliv2.zip" \
    && unzip awscliv2.zip \
    && ./aws/install

RUN wget -O- https://apt.releases.hashicorp.com/gpg | gpg --dearmor > /usr/share/keyrings/hashicorp-archive-keyring.gpg \
    && echo "deb [signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(. /etc/os-release && echo $VERSION_CODENAME) main" > /etc/apt/sources.list.d/hashicorp.list \
    && apt-get update \
    && apt-get install -y terraform

COPY ./app/Cargo.toml /app/Cargo.toml
COPY ./app/Cargo.lock /app/Cargo.lock
COPY ./app/src/bin/helloworld.rs /app/src/bin/helloworld.rs

WORKDIR /app

RUN cargo build --release --bin helloworld

COPY ./app /app
COPY ./terraform /app/terraform

RUN cargo build --release
RUN cargo build --release --bin full_result_parser_local_html
RUN cargo build --release --bin full_result_html_dir_parser
RUN cargo build --release --bin racecards_time_order_scraper
RUN cargo build --release --bin racecard_html_dir_parser
RUN cargo build --release --bin today_first_race_table
RUN cargo build --release --bin backtest

COPY ./scripts/runscript.sh /app/runscript.sh
RUN chmod +x /app/runscript.sh

COPY ./scripts/process_captured_s3.sh /app/process_captured_s3.sh
RUN chmod +x /app/process_captured_s3.sh

COPY ./scripts/trigger_processor_months.sh /app/trigger_processor_months.sh
RUN chmod +x /app/trigger_processor_months.sh

COPY ./scripts/backfill_last_2_years.sh /app/backfill_last_2_years.sh
RUN chmod +x /app/backfill_last_2_years.sh

COPY ./scripts/backfill_jun_2023_to_jun_2024_no_processor.sh /app/backfill_jun_2023_to_jun_2024_no_processor.sh
RUN chmod +x /app/backfill_jun_2023_to_jun_2024_no_processor.sh

COPY ./scripts/scrape_racecard.sh /app/scrape_racecard.sh
RUN chmod +x /app/scrape_racecard.sh

ENTRYPOINT ["bash", "./runscript.sh"]

