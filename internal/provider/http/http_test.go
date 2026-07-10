package httpprovider

import (
	"context"
	"fmt"
	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"
)

func image(t *testing.T) string {
	t.Helper()
	f := t.TempDir() + "/a.png"
	os.WriteFile(f, []byte("\x89PNG\r\n\x1a\nbody"), 0600)
	return f
}
func TestUploadFieldsHeadersJSONPath(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("X-Key") != "value" {
			t.Error("missing header")
		}
		if e := r.ParseMultipartForm(1 << 20); e != nil {
			t.Error(e)
		}
		if r.FormValue("folder") != "images" {
			t.Error("missing field")
		}
		f, _, e := r.FormFile("asset")
		if e != nil {
			t.Error(e)
		} else {
			io.Copy(io.Discard, f)
			f.Close()
		}
		fmt.Fprint(w, `{"data":{"url":"https://cdn.test/a.png"}}`)
	}))
	defer s.Close()
	p := New("custom", config.ProviderConfig{URL: s.URL, FileField: "asset", URLJSONPath: "data.url", Headers: map[string]string{"X-Key": "value"}, Fields: map[string]string{"folder": "images"}}, nil)
	r, e := p.Upload(context.Background(), model.UploadRequest{LocalPath: image(t), RemotePath: "a.png", ContentType: "image/png"})
	if e != nil {
		t.Fatal(e)
	}
	if r.URL != "https://cdn.test/a.png" {
		t.Fatal(r.URL)
	}
}
func TestErrorsAreBoundedAndSecretSafe(t *testing.T) {
	secret := "Bearer private-token"
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(500)
		fmt.Fprint(w, secret+strings.Repeat("x", 2000))
	}))
	defer s.Close()
	p := New("x", config.ProviderConfig{URL: s.URL, URLJSONPath: "url", Headers: map[string]string{"Authorization": secret}}, nil)
	_, e := p.Upload(context.Background(), model.UploadRequest{LocalPath: image(t)})
	if e == nil || strings.Contains(e.Error(), secret) || len(e.Error()) > 700 {
		t.Fatalf("unsafe error: %v", e)
	}
}

func TestOversizedResponse(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, strings.Repeat("x", maxResponse+1))
	}))
	defer s.Close()
	p := New("x", config.ProviderConfig{URL: s.URL, URLJSONPath: "url"}, nil)
	_, err := p.Upload(context.Background(), model.UploadRequest{LocalPath: image(t)})
	if err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("expected bounded response error: %v", err)
	}
}
func TestCancel(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { <-r.Context().Done() }))
	defer s.Close()
	p := New("x", config.ProviderConfig{URL: s.URL, URLJSONPath: "url"}, &http.Client{Timeout: time.Second})
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, e := p.Upload(ctx, model.UploadRequest{LocalPath: image(t)})
	if e == nil {
		t.Fatal("expected cancellation")
	}
}
