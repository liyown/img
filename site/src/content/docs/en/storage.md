---
title: "Storage configuration"
description: "Choose providers directly and understand credentials, paths, and configuration precedence."
locale: en
topic: storage
order: 1
---
## Configure storage in the GUI

Settings → Storage supports create, edit, test, set default, and remove. R2, S3, OSS, GitHub, and HTTP have dedicated forms. New credentials go into macOS Keychain; the configuration stores references. Leaving a credential field blank while editing preserves its current value.

The GUI maintains configuration for you. The CLI offers interactive `img init`, management commands, and TOML files.

## R2 and S3

Set `IMG_R2_ACCESS_KEY` and `IMG_R2_SECRET_KEY` in your terminal environment, then run:

```sh
img init --type s3 --name r2 \
  --endpoint https://ACCOUNT_ID.r2.cloudflarestorage.com \
  --region auto --bucket images \
  --access-key '${IMG_R2_ACCESS_KEY}' \
  --secret-key '${IMG_R2_SECRET_KEY}' \
  --public-url https://images.example.com --path-style
img provider use r2
img config validate
```

Single quotes preserve environment references instead of writing secrets into the configuration. Replace the account, bucket, and public domain. Generic S3 uses your provider's endpoint and region; OSS uses its regional S3-compatible endpoint. `public_url` must serve uploaded objects publicly. An API endpoint is not the same as an image's public address.

## GitHub

Set `IMG_GITHUB_TOKEN` in your environment, then run:

```sh
img init --type github --name github \
  --owner YOUR_NAME --repo images --branch main \
  --token '${IMG_GITHUB_TOKEN}'
```

The token needs content write access to the destination repository. You can configure `public_url` and `commit_message`; the default message is `upload: {path}`.

## Custom HTTP

```sh
img init --type http --name custom \
  --url https://example.com/api/upload \
  --method POST --file-field file --url-json-path data.url
```

`url_json_path` locates the public image address in the JSON response. POST, PUT, and PATCH are supported. Add headers and form fields through the GUI or TOML:

```toml
[providers.custom.headers]
Authorization = "Bearer ${IMG_HTTP_TOKEN}"

[providers.custom.fields]
folder = "images"
```

Enable `allow_insecure` only for a trusted HTTP storage service. The source-image `--allow-insecure` flag and storage endpoint configuration are separate controls.

## Locations and precedence

Run `img config path` for the exact location. The macOS default is `~/Library/Application Support/img/config.toml`; Linux uses `img/config.toml` under the XDG configuration directory; Windows uses the user's Roaming configuration directory.

Precedence, low to high: global configuration → current-directory `.img.toml` → environment → command options. `--config` selects a different global file; the CLI still reads the current project's file. The GUI invokes its bundled CLI in an isolated directory and does not read project configuration.

Project `.img.toml` accepts only a version, existing provider, output format, and upload paths. Credentials and `output.copy` are not allowed:

```toml
version = 1
provider = "r2"
[output]
format = "markdown"
[upload]
path = "posts"
path_template = "{year}/{month}/{filename}"
```

Environment overrides: `IMG_PROVIDER`, `IMG_DEFAULT_PROVIDER`, `IMG_OUTPUT_FORMAT`, `IMG_OUTPUT_COPY`, and `IMG_UPLOAD_CONCURRENCY`. Credentials support `${VARIABLE}` references. On macOS, the CLI can also resolve Keychain references created by the GUI; environment values take precedence.

## Management and defaults

```sh
img provider list
img provider show r2
img provider test r2
img provider use r2
img provider remove old
img config list
img config get upload.max_size
img config set upload.retry_count 3
img config unset upload.max_width
```

`provider show` and `config list` redact sensitive fields. Connection testing may write a test object; use storage you control.

CLI core defaults: concurrency 4, per-image limit 20 MiB, and no automatic retries. The GUI saves separate upload preferences, with concurrency 3 and an 8 MiB default limit. The default path template is `{year}/{month}/{filename}`; names are preserved and conflicts fail unless overwrite is enabled. See [config.example.toml](https://github.com/liyown/img/blob/main/config.example.toml) for configuration fields.
