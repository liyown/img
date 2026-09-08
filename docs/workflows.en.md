# Upload, migration and data management

This guide covers upload and data management in 0.4. See the [0.4 guide](0.4.en.md) for the unified library, single-image tools, sync, and reference repair.

## Bring your existing storage

Use Storage → Import PicGo / PicList settings. Review supported profiles, name conflicts and skipped plugins before saving each profile. Existing names receive a suffix; the source JSON is never changed. GitHub, Alibaba OSS and PicList AWS S3 profiles are supported, including multiple configurations. Plugin scripts are never executed.

```sh
img import-config piclist.json
img import-config piclist.json --apply
img provider test imported-name
img provider use imported-name
```

Preview does not print credentials. Applying saves credentials through the system keychain. Test the connection before making the imported profile your default.

## One local library

Successful CLI, editor server, screenshot, document migration, and Agent uploads immediately enter the shared SQLite library. Desktop does not need to be running. Legacy queues and inbox records import idempotently. If saving a new record fails, the upload still returns its successful URL with an explicit record warning.

```sh
img upload image.png --origin agent --format json
img upload private.png --no-history
```

--no-history disables the record and image copy for that upload. Set IMG_DATA_DIR to use a shared isolated directory. Manual desktop imports enter the queue; global quick-upload shortcuts upload immediately and copy successful links.

## Reuse links and diagnose access

Enable reuse in upload settings or pass --reuse. Matching processed content can reuse a link within the same storage configuration, directory and naming rules. Reuse is off by default; an explicit new name bypasses it.

```sh
img upload image.png --reuse
img upload image.png --reuse --force
img check https://img.example.com/image.png
```

--force bypasses reuse; overwriting an existing remote name still requires --overwrite or a different naming strategy. Public checks send no storage credentials and distinguish access denial, missing files, rate limits, service errors and non-image responses. The gallery link menu also offers this check.

Deleting remotely through img invalidates reuse for that destination. After deleting through another tool, force a new upload: reuse does not verify remote availability.

## Processing and watermarks

Settings → Upload & image processing offers Original, Web images and Photo presets, a longest-edge limit, PNG / JPEG / WebP output, JPEG quality and an image watermark. Save & preview compares the original and processed result without uploading.

```sh
img process photo.png --preset web --output preview.webp
img upload photo.png --image-format jpeg --quality 85 --max-edge 2400
img upload photo.png --watermark /absolute/path/logo.png --watermark-opacity 60
```

Legacy upload processing keeps the aspect ratio, avoids enlargement, composites JPEG onto white, and uses lossless WebP. The standalone tools and ProcessingPlan additionally support JPEG background colors and lossy WebP quality or target-size modes. The bottom-right watermark fits within a quarter of the image dimensions, with 60% default opacity.

Explicit processing supports PNG, JPEG and static WebP. Original GIF, animated WebP, SVG and AVIF uploads preserve their bytes; unsupported explicit conversions fail instead of silently discarding animation. Original files are never modified. Explicit CLI processing options replace the global recipe.

## Markdown migration

Settings → Documents & directories previews references and missing local files before migrating a Markdown document. Successful references are replaced; failed references remain. The document directory receives an original backup and a per-image result report.

```sh
img rewrite article.md --dry-run
img rewrite article.md --report article-results.json
img restore-document article.md.img-backup-UUID article.md
```

The desktop Restore document backup action lets you choose an img backup and confirm restoration. Backups use unique names. Restoring also backs up the current document. --stdout prints the rewritten document without modifying its source. A report must use a new path.

## Folders and watching

Desktop folder selection and dropping recursively collect up to 50 images per batch. CLI --recursive supports up to 10,000 images. Symbolic links are not followed.

```sh
img upload ./screenshots --recursive --format json
img watch ./screenshots --reuse
img watch ./screenshots --new-only --interval 2
```

Watching waits for two matching size and modification-time observations before uploading. Only successful versions are marked processed. --new-only skips files already present at startup. Desktop watching continues after closing the window, stops on quitting and does not restart automatically. Watched directories must be separate from application data to avoid uploading cached images in a loop.

## Back up local data

Choose Back up settings & library, select a destination, then choose settings and records, optionally adding cache and credentials. Backups are local directories with SHA-256 manifests.

```sh
img backup ./img-backup
img backup ./img-full-backup --include-cache --include-credentials
img restore ./img-backup
img restore ./img-backup --apply
```

Restore without --apply only verifies and previews. Close desktop before applying from the CLI. The desktop restore action saves the queue, quits, restores and reopens the app.

Image cache and plaintext credentials are excluded by default. Records without cache retain links. Linked remote objects can be downloaded again for preview; unlinked legacy records may require a local original. Credential backups are explicitly opt-in and **unencrypted**; keep them private.

Restoring replaces included settings and records, restores cache files and keeps unrelated files. A recovery copy named img-before-restore-UUID is created first. It preserves exact original settings, including any plaintext keys in legacy configurations. When importing credentials, it also backs up the keychain entries referenced by your current configuration. Keep recovery copies private. A failed restore attempts rollback and reports the recovery location. Remote storage and original image files are unchanged.

## Remote files and WebDAV

Select an index scope in Storage, then use the unified library to search, preview, download, and manage remote images. Advanced CLI commands also support directory browsing and single-object deletion. Supported backends are S3-compatible storage, GitHub and WebDAV. Custom HTTP upload APIs do not have a shared browsing or deletion protocol.

```sh
img remote --provider r2 list --prefix posts/
img remote --provider r2 list --prefix posts/ --cursor TOKEN
img remote --provider r2 delete posts/image.png --version '"ETAG"' --yes
```

Deletion requires the ETag or GitHub SHA returned by the listing. Changed files must be refreshed and confirmed again. Directory deletion is disabled. Remote deletion affects existing links. Clearing caches preserves the library; hiding records preserves remote images. Versioned S3 buckets may retain old versions behind a delete marker.

GitHub directories at the Contents API's 1,000-entry limit return a clear error; browse smaller subdirectories. Remote browsing does not download full-size images as thumbnails.

```sh
img init --type webdav --name dav --endpoint https://dav.example.com/images/ --public-url https://img.example.com --authorization '${DAV_AUTHORIZATION}'
```

WebDAV supports authenticated endpoints through an Authorization header reference, automatic directory creation and conditional uploads that do not overwrite by default. The public image URL must be accessible to readers without WebDAV login.

## Appearance and platform support

Dark mode changes immediately and persists. English and Simplified Chinese preferences apply when reopening img. Native file pickers also follow system language settings.

Windows adds a draggable region screenshot picker with Escape to cancel, plus a tray menu. Linux Wayland uses desktop portals for screenshots and global shortcuts; shortcut availability depends on GlobalShortcuts portal support. Linux trays require StatusNotifier support. macOS retains its menu bar and region screenshot flow.
