FROM rust:1.96-bullseye

COPY rust-services/das-system /das-system
WORKDIR /das-system
ENV INIT_FILE_PATH=/das-system/init.sql
RUN cargo build --release -p migration
CMD /das-system/target/release/migration up
