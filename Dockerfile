FROM rust:trixie 

RUN apt-get update
RUN apt-get install -y net-tools inetutils-tools inetutils-ping nano unzip gpg wget xvfb chromium
RUN apt-get install -y curl

RUN mv /usr/bin/chromium /usr/bin/og-chromium \
    && ln -s /usr/bin/og-chromium /usr/bin/chromium

RUN curl "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o "awscliv2.zip" \
    && unzip awscliv2.zip \
    && ./aws/install

COPY ./app/Cargo.toml /app/Cargo.toml
COPY ./app/Cargo.lock /app/Cargo.lock
COPY ./app/src/bin/helloworld.rs /app/src/bin/helloworld.rs

WORKDIR /app

COPY ./scripts/runscript.sh /app/runscript.sh
RUN chmod +x /app/runscript.sh

RUN cargo build --release --bin helloworld

COPY ./app /app

RUN cargo build --release

COPY ./scripts/backfill_last_2_years.sh /app/backfill_last_2_years.sh
RUN chmod +x /app/backfill_last_2_years.sh

ENTRYPOINT ["bash", "./runscript.sh"]

