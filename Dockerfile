# Minimal runtime image — binary is built on the host and injected.
# Usage: docker build --build-arg BINARY=path/to/ozzie .
FROM alpine:3.21

ARG BINARY=ozzie

RUN apk add --no-cache \
    git curl jq bash openssh-client \
    docker-cli ca-certificates age \
    sqlite-libs

COPY ${BINARY} /usr/local/bin/ozzie
RUN chmod +x /usr/local/bin/ozzie

COPY scripts/detect-tools.sh /tmp/detect-tools.sh
RUN mkdir -p /etc/ozzie && \
    sh /tmp/detect-tools.sh > /etc/ozzie/system-tools.json && \
    rm /tmp/detect-tools.sh

ENV OZZIE_RUNTIME=container

RUN adduser -D -h /home/ozzie ozzie
USER ozzie

ENV OZZIE_PATH=/home/ozzie/.ozzie
VOLUME ["/home/ozzie/.ozzie"]

EXPOSE 18420
ENTRYPOINT ["ozzie"]
CMD ["gateway", "--host", "0.0.0.0"]
