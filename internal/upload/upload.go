package upload

import (
	"context"
	"fmt"
	"path/filepath"
	"sync"
	"time"

	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/filetype"
	"github.com/liyown/img/internal/model"
	"github.com/liyown/img/internal/pathgen"
)

type FileResult struct {
	LocalPath   string `json:"local_path"`
	Success     bool   `json:"success"`
	RemotePath  string `json:"remote_path,omitempty"`
	URL         string `json:"url,omitempty"`
	Provider    string `json:"provider,omitempty"`
	Size        int64  `json:"size,omitempty"`
	ContentType string `json:"content_type,omitempty"`
	Error       string `json:"error,omitempty"`
}
type Options struct {
	Path, Name string
	Overwrite  bool
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
	typ, size, err := filetype.Inspect(file, c.MaxSize)
	if err != nil {
		r.Error = err.Error()
		return r
	}
	now := o.Now
	if now.IsZero() {
		now = time.Now()
	}
	template := c.PathTemplate
	if o.Name != "" {
		if filepath.Base(o.Name) != o.Name {
			r.Error = "--name must be a file name without directories"
			return r
		}
		template = o.Name
	}
	remote, err := pathgen.Generate(file, template, choose(o.Path, c.Path), c.Rename, now)
	if err != nil {
		r.Error = err.Error()
		return r
	}
	u, err := p.Upload(ctx, model.UploadRequest{LocalPath: file, RemotePath: remote, ContentType: typ, Overwrite: o.Overwrite || c.Overwrite})
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
	return r
}
func choose(a, b string) string {
	if a != "" {
		return a
	}
	return b
}
