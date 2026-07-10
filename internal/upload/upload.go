package upload

import (
	"context"
	"fmt"
	"io"
	"os"
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
	body := io.NewSectionReader(f, 0, size)
	u, err := p.Upload(ctx, model.UploadRequest{LocalPath: file, FileName: filepath.Base(file), Body: body, Size: size, RemotePath: remote, ContentType: typ, Overwrite: overwrite})
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
