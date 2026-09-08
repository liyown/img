---
title: 'Desktop guide'
description: 'Use the library, queue, copy formats, settings, and background quick actions.'
locale: en
topic: desktop
order: 2
---

This guide describes the published stable release. Main-branch additions such as the shared library, dark mode, folder imports and remote management are covered in [migration and management](/img/en/docs/workflows/); release verification is still in progress.

## Import and upload

The main window accepts selected files, drops, clipboard images, screenshots, and multiple image URLs. Imports enter the ready queue. Click Upload to start. Each batch captures its provider, processing options, and copy preferences at launch.

Progress follows actual request-body reads. After transmission, the app waits for server confirmation before marking success. Pause, resume, cancel a file or batch, and retry failures. Pausing terminates the request; resuming uploads the file again. This is **not byte-level resumable transfer**.

## Library and links

The library shows successful uploads in grid or list view; history retains finished records. Search names, providers, or URLs. Click a thumbnail to enlarge it; Escape closes the preview.

Copy as URL, Markdown image, Markdown link, HTML, or BBCode. Automatic copying is enabled by default and includes successful batch links in import order, separated by newlines. Clearing records deletes app copies and records only, preserving original files and remote images.

## Settings sections

| Section               | Controls                                                                              |
| --------------------- | ------------------------------------------------------------------------------------- |
| Storage               | Create, edit, test, default provider, credentials                                     |
| Upload and processing | Concurrency, retries, compression, maximum width, JPEG EXIF removal, paths, conflicts |
| Links and clipboard   | Copy format and automatic copying                                                     |
| App preferences       | Shortcuts and interface preferences                                                   |
| About and updates     | Version, diagnostics, CLI entry point, manual update checks                           |

The stable release does not include dark mode, remote library management, tags, or folder imports. Its UI is Chinese. See the development guide above for main-branch additions.

## macOS window and quick actions

Closing the window or pressing ⌘W hides it while uploads continue. ⌘Q exits; during uploads you can wait for completion or pause and exit. The menu bar offers Open, Upload clipboard, Capture and upload, Pause/resume, and Quit.

| Shortcut | Action                                 |
| -------- | -------------------------------------- |
| ⌘U       | Select images and enqueue              |
| ⌘V       | Import clipboard content               |
| ⌘⇧S      | Capture and enqueue                    |
| ⌘F       | Search                                 |
| ⌘B       | Collapse or expand the sidebar         |
| ⌘⌥U      | Global: upload clipboard immediately   |
| ⌘⌥S      | Global: capture and upload immediately |

Global shortcuts can be changed or disabled. Quick batches run in order without including unsubmitted manual imports. Without a default provider, content is retained and storage settings open. Cancelling capture creates no task. Each batch produces one result notification.

Global shortcut delivery, third-party conflicts, system capture cancellation, status-item clicks, and notification permissions and clicks still need manual testing for the stable release. See the [acceptance record](https://github.com/liyown/img/blob/main/stability-qa.md).

## Installation, terminal command and updates

On macOS, drag the official Img.app from the DMG into Applications, or choose Install and reopen in About and updates to install it at `~/Applications/Img.app`. Development and test apps do not offer this action; use an official package from the [installation page](/img/en/install/#gui).

Add terminal command uses `~/.local/bin` by default and does not overwrite another img command. Add this directory to PATH if your terminal cannot find img, or use the standalone CLI installer. Windows uses the current user's Programs/img directory.

Check for updates checks stable desktop releases only. Downloads are verified for architecture, size and SHA-256 before installation. Community builds may need system permission to open after an update.

## Local data and recovery

Data lives in `~/Library/Application Support/aperture`, preserving the historical directory through upgrades. It contains the queue, image copies, thumbnails, and preferences. Only one process can use a data directory at a time.

Queue saves are serialized and atomic, with two recent valid backups. If corruption is detected, restore a backup or preserve the damaged file and rebuild an empty queue. Uploads stay blocked until recovery. See [troubleshooting](/img/en/docs/troubleshooting/).
