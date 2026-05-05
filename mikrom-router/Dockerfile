FROM caddy:2.11-builder AS builder

WORKDIR /src
COPY . /src/mikrom-router

RUN xcaddy build \
    --with github.com/antpard/mikrom/mikrom-router=/src/mikrom-router

FROM caddy:2.11

COPY --from=builder /usr/bin/caddy /usr/bin/caddy

# Default Caddyfile
COPY <<EOF /etc/caddy/Caddyfile
{
    order mikrom_router before reverse_proxy
    mikrom_app {
        nats_url {$NATS_URL}
        db_url {$DATABASE_URL}
        master_key {$MASTER_KEY}
    }
}

:80 {
    mikrom_router
    reverse_proxy {vars.mikrom_target}
}
EOF

CMD ["caddy", "run", "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile"]
