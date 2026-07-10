package upload

import (
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/liyown/img/internal/config"
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
	Path, Name string
	Overwrite  bool
	Optimize   bool
	Now        time.Time
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

func one(ctx context.Context, p model.Provider, c config.Upload, file string, o Options) FileResult {
	r := FileResult{LocalPath: file}
	f, err := os.Open(file)
	if err != nil {
		r.Error = fmt.Sprintf("open image %s: %v", file, err)
		return r
	}
	defer f.Close()
	typ, size, err := filetype.InspectFile(f, file, c.MaxSize)
	if err != nil {
		r.Error = err.Error()
		return r
	}
	now := o.Now
	if now.IsZero() {
		now = time.Now()
	}

	// Optimise before path generation so the remote extension can reflect the
	// (possibly converted) output format.
	var body io.ReadSeeker = io.NewSectionReader(f, 0, size)
	var originalSize int64
	if o.Optimize {
		res, oerr := optimize.TryCompress(io.NewSectionReader(f, 0, size), typ, size)
		if oerr == nil && res.Reduced {
			body = res.Body
			originalSize = res.OriginalSize
			size = res.Size
			typ = res.ContentType
			// If format changed (e.g. PNG→JPEG), update the local-path hint used
			// for remote-path generation so {ext} and {filename} stay consistent.
			if res.ContentType == "image/jpeg" && !strings.HasSuffix(strings.ToLower(file), ".jpg") && !strings.HasSuffix(strings.ToLower(file), ".jpeg") {
				ext := filepath.Ext(file)
				file = strings.TrimSuffix(file, ext) + ".jpg"
			}
		} else if oerr == nil {
			// No gain; body stays as-is, re-seek original file section.
			body = io.NewSectionReader(f, 0, size)
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
	remote, err := pathgen.GenerateFromReader(file, f, template, choose(o.Path, c.Path), c.Rename, now)
	if err != nil {
		r.Error = err.Error()
		return r
	}
	overwrite := o.Overwrite || c.Overwrite || c.Conflict == "overwrite"
	u, err := p.Upload(ctx, model.UploadRequest{
		LocalPath:   r.LocalPath, // always original local path for logging
		FileName:    filepath.Base(file),
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
