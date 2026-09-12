FROM rust:1.96-bullseye  AS builder
RUN apt-get update -y && \
    apt-get install -y build-essential make git
COPY rust-services/das-system/digital_asset_types /das-system/digital_asset_types

COPY rust-services/das-system/ /das-system/

WORKDIR /das-system/tools/acc_forwarder
# Build application
RUN cargo build --release

FROM rust:1.96-slim-bullseye
ARG APP=/usr/src/app
RUN apt update \
    && apt install -y curl ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*
ENV TZ=Etc/UTC \
    APP_USER=appuser
RUN groupadd $APP_USER \
    && useradd -g $APP_USER $APP_USER \
    && mkdir -p ${APP}
COPY --from=builder /das-system/target/release/acc_forwarder ${APP}
RUN chown -R $APP_USER:$APP_USER ${APP}
USER $APP_USER
WORKDIR ${APP}
CMD /usr/src/app/acc_forwarder
