package upload

import (
	"bytes"
	"context"
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
	// Apply lossless EXIF stripping and/or downscaling before the format
	// optimiser so that each step can work on already-processed bytes.
	body := newReader()
	var originalSize int64

	// Step 1: EXIF strip (lossless, JPEG only).
	if o.StripEXIF && typ == "image/jpeg" {
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
	if o.MaxWidth > 0 {
		res, err := optimize.ScaleDown(newReader(), typ, size, o.MaxWidth, 0)
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
	u, err := p.Upload(ctx, model.UploadRequest{
		LocalPath:   r.LocalPath, // original source (path or URL) for logging
		FileName:    filepath.Base(name),
		Body:        body,
		Size:        size,
		RemotePath:  remote,
		ContentType: typ,
		Overwrite:   overwrite,
	})
	if err != nil {
		r.Error = fmt.Sprintf("upload with provider %s: %v", p.Name(), err)
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
