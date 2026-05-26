# mikrom-router

`mikrom-router` es el borde HTTP/TLS de Mikrom. Ejecuta el proxy, el control plane, la publicación de tráfico y la capa de telemetría. También coordina el estado persistido de rutas, certificados, ACME y WireGuard.

## Componentes

- `app/`: bootstrap, config y runtime.
- `domain/`: salud, estado y contratos de dominio.
- `application/`: orquestación de control plane, proxy, tráfico y telemetría.
- `infrastructure/`: NATS, persistencia, TLS, CA, WireGuard y crypto.

## Dependencias operativas

- PostgreSQL, usado por el control plane.
- NATS, usado para control plane, telemetría y tráfico.
- WireGuard en el kernel, más permisos `CAP_NET_ADMIN`, para crear y mantener la interfaz.
- Un directorio de CA upstream si vas a hacer TLS hacia upstreams con CA propia.
- Opcionalmente un endpoint OTLP para logs, trazas y métricas.

## Configuración

`RouterConfig` se carga desde entorno con `dotenvy` y valida combinaciones críticas antes de arrancar.

Variables principales:

- `DATABASE_URL`
- `NATS_URL`
- `NATS_USE_TLS`
- `NATS_CERTS_DIR` o `CERTS_DIR`
- `UPSTREAM_CA_CERTS_DIR`
- `MASTER_KEY`
- `ROUTER_ID`
- `ADVERTISE_ADDRESS`
- `DATA_DIR`
- `STATE_CACHE_PATH`
- `WIREGUARD_PORT`
- `ACME_STAGING`
- `RPS_LIMIT`
- `ROUTER_THREADS`

Valores por defecto relevantes:

- `ROUTER_ID`: hostname local, o `unknown-router` si no puede resolverse.
- `ADVERTISE_ADDRESS`: `ROUTER_ID` si no se define explícitamente.
- `DATA_DIR`: `/var/lib/mikrom`
- `STATE_CACHE_PATH`: `${DATA_DIR}/router-state.json`
- `WIREGUARD_PORT`: `51822`
- `RPS_LIMIT`: `100`
- `ROUTER_THREADS`: CPUs disponibles, mínimo `1`
- `NATS_USE_TLS`: `false`
- `ACME_STAGING`: `false`

Validaciones importantes:

- `MASTER_KEY` no puede estar vacío.
- `ADVERTISE_ADDRESS` no puede estar en blanco.
- `RPS_LIMIT` debe ser mayor que cero.
- `ROUTER_THREADS` debe ser mayor que cero.
- `WIREGUARD_PORT` debe ser mayor que cero.
- Si `NATS_USE_TLS=true`, debe existir `NATS_CERTS_DIR` o `CERTS_DIR`.
- `NATS_CERTS_DIR`, `UPSTREAM_CA_CERTS_DIR` y `STATE_CACHE_PATH` no pueden apuntar a rutas vacías o inválidas.

## Health endpoints

El proxy expone los siguientes endpoints:

- `GET /health/live`
- `GET /health/ready`
- `GET /health/deps`
- `GET /health/control-plane`

Semántica:

- `/health/live`: el proceso sigue vivo.
- `/health/ready`: el router puede servir tráfico de forma completa.
- `/health/deps`: dependencias críticas iniciales levantadas.
- `/health/control-plane`: el control plane completó la sincronización inicial.

El estado interno puede ser:

- `Booting`
- `Degraded`
- `Ready`
- `ShuttingDown`
- `Fatal`

## Arranque y recuperación

El bootstrap hace lo siguiente, en orden:

1. Inicializa tracing.
2. Carga el estado persistido desde `STATE_CACHE_PATH`.
3. Arranca el control plane, la telemetría y la publicación de tráfico.
4. Carga la CA upstream si está configurada.
5. Levanta el proxy HTTP/TLS.

Comportamiento de recuperación:

- Si el cache de estado está corrupto, el router cae a un estado vacío seguro.
- Si NATS o DB fallan de forma transitoria, los componentes usan reintentos con backoff.
- Si el control plane recupera la sincronización, la salud puede pasar de `Degraded` a `Ready`.
- Si el proceso entra en `ShuttingDown` o `Fatal`, `/health/live` deja de ser `200`.

## Observabilidad

- SigNoz OTLP gRPC: `OTEL_EXPORTER_OTLP_ENDPOINT` (`http://192.168.122.128:4317` por defecto)
- Las métricas operativas se exportan por OTLP e incluyen estado de salud, contadores HTTP, ACME, redirecciones y latencia media.
- El router ya no expone un `GET /metrics` local; esa información sale por OpenTelemetry hacia SigNoz.

## Desarrollo local

- Usa `cargo test -p mikrom-router` para la validación normal.
- Usa `cargo clippy -p mikrom-router --all-targets` para revisión estática.
- El binario espera poder crear su directorio de datos y el archivo de cache si no existen.
