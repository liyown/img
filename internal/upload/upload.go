package upload

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/fetch"
	"github.com/liyown/img/internal/filetype"
	"github.com/liyown/img/internal/model"
	"github.com/liyown/img/internal/optimize"
	"github.com/liyown/img/internal/pathgen"
)

type FileResult struct {
	LocalPath    string `json:"local_path"`
	Success      bool   `json:"success"`
	RemotePath   string `json:"remote_path,omitempty"`
	URL          string `json:"url,omitempty"`
	Provider     string `json:"provider,omitempty"`
	Size         int64  `json:"size,omitempty"`
	OriginalSize int64  `json:"original_size,omitempty"` // set only when --optimize reduces size
	ContentType  string `json:"content_type,omitempty"`
	Error        string `json:"error,omitempty"`
}

type Options struct {
	Path, Name    string
	Overwrite     bool
	Optimize      bool
	StripEXIF     bool // strip EXIF/XMP metadata from JPEG before upload
	MaxWidth      int  // downscale to fit this width (px); 0 = no resize
	AllowInsecure bool // permit fetching source URLs over plain HTTP
	Now           time.Time
}

func Run(ctx context.Context, p model.Provider, c config.Upload, files []string, o Options) []FileResult {
	res := make([]FileResult, len(files))
	workers := c.Concurrency
	if workers > len(files) {
		workers = len(files)
	}
	jobs := make(chan int)
	var wg sync.WaitGroup
	for range workers {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := range jobs {
				res[i] = one(ctx, p, c, files[i], o)
			}
		}()
	}
	for i := range files {
		select {
		case jobs <- i:
		case <-ctx.Done():
			res[i] = FileResult{LocalPath: files[i], Error: ctx.Err().Error()}
		}
	}
	close(jobs)
	wg.Wait()
	return res
}

func one(ctx context.Context, p model.Provider, c config.Upload, src string, o Options) FileResult {
	r := FileResult{LocalPath: src}

	// A "source" is either a local path or a remote URL. Both resolve to a
	// detected type, a size, a display name (used for {filename}/{ext}), and a
	// factory that yields a fresh independent reader over the content.
	var typ string
	var size int64
	var name string
	var newReader func() io.ReadSeeker

	if fetch.IsURL(src) {
		got, err := fetch.Fetch(ctx, src, c.MaxSize, o.AllowInsecure, nil)
		if err != nil {
			r.Error = err.Error()
			return r
		}
		typ, size, err = filetype.InspectBytes(got.Data, got.FileName, c.MaxSize)
		if err != nil {
			r.Error = err.Error()
			return r
		}
		name = got.FileName
		data := got.Data
		newReader = func() io.ReadSeeker { return bytes.NewReader(data) }
	} else {
		f, err := os.Open(src)
		if err != nil {
			r.Error = fmt.Sprintf("open image %s: %v", src, err)
			return r
		}
		defer f.Close()
		typ, size, err = filetype.InspectFile(f, src, c.MaxSize)
		if err != nil {
			r.Error = err.Error()
			return r
		}
		name = src
		origSize := size // capture before optimize can reassign size
		newReader = func() io.ReadSeeker { return io.NewSectionReader(f, 0, origSize) }
	}

	now := o.Now
	if now.IsZero() {
		now = time.Now()
	}

	// ── Pre-processing ────────────────────────────────────────────────────────
	// Merge config-level defaults with per-call option overrides.
	doStripEXIF := o.StripEXIF || c.StripEXIF
	doMaxWidth := o.MaxWidth
	if doMaxWidth == 0 {
		doMaxWidth = c.MaxWidth
	}

	body := newReader()
	var originalSize int64

	// Step 1: EXIF strip (lossless, JPEG only).
	if doStripEXIF && typ == "image/jpeg" {
		if raw, err := io.ReadAll(newReader()); err == nil {
			if stripped, changed := optimize.StripJPEGMetadata(raw); changed {
				if originalSize == 0 {
					originalSize = size
				}
				size = int64(len(stripped))
				captured := stripped
				newReader = func() io.ReadSeeker { return bytes.NewReader(captured) }
				body = newReader()
			}
		}
	}

	// Step 2: Scale down (decode → CatmullRom → encode as JPEG/WebP).
	if doMaxWidth > 0 {
		res, err := optimize.ScaleDown(newReader(), typ, size, doMaxWidth, 0)
		if err == nil && res.Reduced {
			if originalSize == 0 {
				originalSize = res.OriginalSize
			}
			size = res.Size
			typ = res.ContentType
			captured, _ := io.ReadAll(res.Body)
			newReader = func() io.ReadSeeker { return bytes.NewReader(captured) }
			body = newReader()
			if newExt := extForType(typ); newExt != "" {
				lower := strings.ToLower(name)
				if !strings.HasSuffix(lower, newExt) && !(newExt == ".jpg" && strings.HasSuffix(lower, ".jpeg")) {
					name = strings.TrimSuffix(name, filepath.Ext(name)) + newExt
				}
			}
		}
	}

	// Step 3: Format compression (JPEG re-encode / PNG→WebP / etc.).
	if o.Optimize {
		res, oerr := optimize.TryCompress(newReader(), typ, size)
		if oerr == nil && res.Reduced {
			if originalSize == 0 {
				originalSize = res.OriginalSize
			}
			body = res.Body
			size = res.Size
			typ = res.ContentType
			if newExt := extForType(res.ContentType); newExt != "" {
				lower := strings.ToLower(name)
				if !strings.HasSuffix(lower, newExt) && !(newExt == ".jpg" && strings.HasSuffix(lower, ".jpeg")) {
					name = strings.TrimSuffix(name, filepath.Ext(name)) + newExt
				}
			}
		}
	}

	template := c.PathTemplate
	if o.Name != "" {
		if filepath.Base(o.Name) != o.Name {
			r.Error = "--name must be a file name without directories"
			return r
		}
		template = o.Name
	}
	remote, err := pathgen.GenerateFromReader(name, newReader(), template, choose(o.Path, c.Path), c.Rename, now)
	if err != nil {
		r.Error = err.Error()
		return r
	}
	overwrite := o.Overwrite || c.Overwrite || c.Conflict == "overwrite"
	req := model.UploadRequest{
		LocalPath:   r.LocalPath,
		FileName:    filepath.Base(name),
		Body:        body,
		Size:        size,
		RemotePath:  remote,
		ContentType: typ,
		Overwrite:   overwrite,
	}

	// ── Upload with retry ─────────────────────────────────────────────────────
	retries := c.RetryCount
	var uploadErr error
	var u *model.UploadResult
	for attempt := 0; attempt <= retries; attempt++ {
		if attempt > 0 {
			// Exponential backoff: 1s, 2s, 4s, …
			delay := time.Duration(1<<uint(attempt-1)) * time.Second
			select {
			case <-ctx.Done():
				r.Error = ctx.Err().Error()
				return r
			case <-time.After(delay):
			}
			// Rewind body for retry.
			if _, se := body.Seek(0, io.SeekStart); se != nil {
				break
			}
		}
		u, uploadErr = p.Upload(ctx, req)
		if uploadErr == nil {
			break
		}
		if !isRetryable(uploadErr) {
			break
		}
	}
	if uploadErr != nil {
		r.Error = fmt.Sprintf("upload with provider %s: %v", p.Name(), uploadErr)
		return r
	}
	r.Success = true
	r.RemotePath = u.RemotePath
	r.URL = u.URL
	r.Provider = u.Provider
	r.Size = size
	r.ContentType = typ
	if originalSize > 0 {
		r.OriginalSize = originalSize
	}
	return r
}

// isRetryable reports whether an upload error is likely transient and worth
// retrying. Permanent errors (already exists, auth, invalid file) are not
// retried regardless of retry_count.
func isRetryable(err error) bool {
	if err == nil {
		return false
	}
	if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return false
	}
	msg := strings.ToLower(err.Error())
	for _, perm := range []string{"already exists", "not found", "unauthorized", "forbidden", "unsupported"} {
		if strings.Contains(msg, perm) {
			return false
		}
	}
	for _, transient := range []string{"rate limit", "timeout", "connection", "temporary", "unavailable", "retry", "too many"} {
		if strings.Contains(msg, transient) {
			return true
		}
	}
	// Retry on generic network/IO errors that aren't clearly permanent.
	return true
}

func choose(a, b string) string {
	if a != "" {
		return a
	}
	return b
}

// extForType maps a content type produced by the optimizer to the file
// extension that should be used for the remote path. Returns "" for types
// that do not require an extension change.
func extForType(contentType string) string {
	switch contentType {
	case "image/jpeg":
		return ".jpg"
	case "image/webp":
		return ".webp"
	default:
		return ""
	}
}
