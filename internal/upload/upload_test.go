package upload

import (
	"context"
	"fmt"
	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
	"os"
	"sync"
	"testing"
	"time"
)

type fake struct {
	mu          sync.Mutex
	active, max int
	fail        string
}

func (f *fake) Name() string                   { return "fake" }
func (f *fake) Type() string                   { return "fake" }
func (f *fake) Validate(context.Context) error { return nil }
func (f *fake) Upload(ctx context.Context, r model.UploadRequest) (*model.UploadResult, error) {
	f.mu.Lock()
	f.active++
	if f.active > f.max {
		f.max = f.active
	}
	f.mu.Unlock()
	defer func() { f.mu.Lock(); f.active--; f.mu.Unlock() }()
	select {
	case <-time.After(10 * time.Millisecond):
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	if r.LocalPath == f.fail {
		return nil, fmt.Errorf("failed")
	}
	return &model.UploadResult{Provider: "fake", LocalPath: r.LocalPath, RemotePath: r.RemotePath, URL: "https://x/" + r.RemotePath, Size: 8, ContentType: r.ContentType}, nil
}
func TestMultiPartialAndConcurrency(t *testing.T) {
	var files []string
	for i := 0; i < 5; i++ {
		p := fmt.Sprintf("%s/%d.png", t.TempDir(), i)
		os.WriteFile(p, []byte("\x89PNG\r\n\x1a\n"), 0600)
		files = append(files, p)
	}
	f := &fake{fail: files[2]}
	c := config.Defaults().Upload
	c.Concurrency = 2
	r := Run(context.Background(), f, c, files, Options{Now: time.Date(2026, 7, 10, 0, 0, 0, 0, time.UTC)})
	if len(r) != 5 || !r[0].Success || r[2].Success || f.max > 2 {
		t.Fatalf("results=%+v max=%d", r, f.max)
	}
}

func TestCancellation(t *testing.T) {
	p := t.TempDir() + "/a.png"
	os.WriteFile(p, []byte("\x89PNG\r\n\x1a\n"), 0600)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	r := Run(ctx, &fake{}, config.Defaults().Upload, []string{p}, Options{})
	if len(r) != 1 || r[0].Success || r[0].Error == "" {
		t.Fatalf("cancellation lost: %+v", r)
	}
}
