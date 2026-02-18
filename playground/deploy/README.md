# Playground deploy notes

## API process

Run the service using Bazel so runfiles are resolved canonically:

```sh
bazel run //playground/api:main
```

The provided systemd unit uses the same `bazel run //playground/api:main`
command.

## Systemd

Install `playground/deploy/playground_api.service`:

```sh
cp playground/deploy/playground_api.service /etc/systemd/system/playground-api.service
systemctl daemon-reload
systemctl enable --now playground-api
```

## Nginx

Install `playground/deploy/nginx.conf` as a server config and reload nginx.

```sh
cp playground/deploy/nginx.conf /etc/nginx/sites-available/coppice-playground
ln -s /etc/nginx/sites-available/coppice-playground /etc/nginx/sites-enabled/coppice-playground
nginx -t && systemctl reload nginx
```
