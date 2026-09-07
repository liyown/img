---
title: "Troubleshooting"
description: "Understand upload errors, recover queues, and export minimal diagnostics."
locale: en
topic: troubleshooting
order: 4
---
## Start with the error category

| Error code | Next step |
| --- | --- |
| `invalid_config` | Check the default provider, required fields, and configuration format |
| `authentication` / `permission` | Check credentials and bucket or repository write access |
| `network` / `timeout` | Check the network and endpoint, then retry |
| `rate_limited` / `server` | Retry later; reduce concurrency if needed |
| `conflict` | Rename the file or deliberately enable overwrite |
| `too_large` | Check local and server limits; compress or resize |
| `invalid_image` | Check for damage and supported image formats |
| `file_not_found` / `io` | Reselect the original file and check read/write permissions |
| `invalid_response` | Check the HTTP response field and public image URL |
| `cancelled` / `unknown` | Review details and current task state |

The GUI offers storage settings, retry, reselect source, and detail actions. Older JSON without new error fields still displays `error`. Network, timeout, rate-limit, and server errors are usually retryable. Check whether the server already received an image before repeating an upload.

## Recover a damaged queue

Uploads stay blocked when corruption is detected. Restore a valid backup, or preserve the damaged file and rebuild an empty queue. Two recent valid backups are kept. Without a valid backup, history cannot be recreated automatically; an empty queue does not restore record mappings.

Do not edit queue files while the app is running. To make a manual backup, quit first and copy `~/Library/Application Support/aperture`.

## Export diagnostics

Export deliberately from the app's diagnostic entry point. The record contains only version, error categories, and operation stages, excluding credentials, request bodies, and URL query parameters. It is not a full log or a queue backup. Add reproduction steps and the OS version when reporting an issue.

## Configuration and CLI

```sh
img version
img config path
img config validate
img provider list
```

For project configuration errors, check the current directory's `.img.toml`. It accepts a restricted set of provider, output-format, and path fields. Global `output.copy` does not belong there.

The GUI can add its bundled CLI through Settings → About and updates → Add terminal command. If your shell cannot find it, add `~/.local/bin` to PATH as prompted. Source `cargo install` normally uses `~/.cargo/bin`.

## Platform and preview limits

Screenshots and clipboard access depend on platform tools and permissions. Linux requires suitable screenshot and clipboard utilities; Windows uses PowerShell for full-screen capture. macOS manages screen-recording and notification permissions. If notifications are denied, results remain available in the queue.

The local GUI package uses development signing and is not Apple-notarized. Formal signing, real cloud uploads, and some native desktop interactions still need acceptance. Read the [acceptance record](https://github.com/liyown/img/blob/main/stability-qa.md), or report reproducible issues on [GitHub](https://github.com/liyown/img/issues).
