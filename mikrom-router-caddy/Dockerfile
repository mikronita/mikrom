FROM caddy:2.11-builder AS builder

WORKDIR /src
COPY mikrom-router ./mikrom-router
COPY mikrom-proto ./mikrom-proto

# Create a custom main module to build Caddy with our plugin
# This avoids xcaddy limitations with multiple local replacements
WORKDIR /src/build
RUN printf 'package main\n\nimport (\n\tcaddycmd "github.com/caddyserver/caddy/v2/cmd"\n\t_ "github.com/caddyserver/caddy/v2/modules/standard"\n\t_ "github.com/antpard/mikrom/mikrom-router"\n)\n\nfunc main() {\n\tcaddycmd.Main()\n}\n' > main.go

RUN go mod init mikrom-caddy && \
    go mod edit -replace github.com/antpard/mikrom/mikrom-router=/src/mikrom-router && \
    go mod edit -replace github.com/antpard/mikrom/mikrom-proto=/src/mikrom-proto/gen/go && \
    go mod tidy

RUN go build -o /usr/bin/caddy main.go

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
