package serve

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
)

// fakeProvider is a minimal model.Provider for testing.
type fakeProvider struct{ baseURL string }

func (f *fakeProvider) Name() string { return "fake" }
func (f *fakeProvider) Type() string { return "fake" }
func (f *fakeProvider) Validate(context.Context) error { return nil }
func (f *fakeProvider) Upload(_ context.Context, r model.UploadRequest) (*model.UploadResult, error) {
	return &model.UploadResult{
		Provider:   "fake",
		LocalPath:  r.LocalPath,
		RemotePath: r.RemotePath,
		URL:        f.baseURL + "/" + r.RemotePath,
		Size:       r.Size,
	}, nil
}

func newServer(t *testing.T) (*Server, string) {
	t.Helper()
	d := t.TempDir()
	img := filepath.Join(d, "test.png")
	os.WriteFile(img, []byte("\x89PNG\r\n\x1a\nbody"), 0600)

	p := &fakeProvider{baseURL: "https://cdn.test"}
	cfg := config.Defaults().Upload
	s := &Server{
		Provider: p,
		Cfg:      cfg,
		Opts:     Options{},
		Out:      io.Discard,
		Err:      io.Discard,
	}
	return s, img
}

func TestHandleUploadJSON(t *testing.T) {
	s, img := newServer(t)

	body, _ := json.Marshal(map[string]any{"list": []string{img}})
	req := httptest.NewRequest(http.MethodPost, "/upload", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	s.Handler().ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", w.Code, w.Body.String())
	}
	var resp picgoResponse
	if err := json.NewDecoder(w.Body).Decode(&resp); err != nil {
		t.Fatal(err)
	}
	if !resp.Success || len(resp.Result) != 1 {
		t.Fatalf("unexpected response: %+v", resp)
	}
	if !strings.HasPrefix(resp.Result[0], "https://cdn.test/") {
		t.Fatalf("URL = %s", resp.Result[0])
	}
}

func TestHandleUploadMultipart(t *testing.T) {
	s, img := newServer(t)

	var buf bytes.Buffer
	mw := multipart.NewWriter(&buf)
	f, _ := os.Open(img)
	part, _ := mw.CreateFormFile("files", "test.png")
	io.Copy(part, f)
	f.Close()
	mw.Close()

	req := httptest.NewRequest(http.MethodPost, "/upload", &buf)
	req.Header.Set("Content-Type", mw.FormDataContentType())
	w := httptest.NewRecorder()

	s.Handler().ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", w.Code, w.Body.String())
	}
	var resp picgoResponse
	json.NewDecoder(w.Body).Decode(&resp)
	if !resp.Success || len(resp.Result) == 0 {
		t.Fatalf("unexpected response: %+v", resp)
	}
}

func TestHandleUploadNoFiles(t *testing.T) {
	s, _ := newServer(t)
	body, _ := json.Marshal(map[string]any{"list": []string{}})
	req := httptest.NewRequest(http.MethodPost, "/upload", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	s.Handler().ServeHTTP(w, req)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("expected 400, got %d", w.Code)
	}
}

func TestHandleWrongMethod(t *testing.T) {
	s, _ := newServer(t)
	req := httptest.NewRequest(http.MethodGet, "/upload", nil)
	w := httptest.NewRecorder()
	s.Handler().ServeHTTP(w, req)
	if w.Code != http.StatusMethodNotAllowed {
		t.Fatalf("expected 405, got %d", w.Code)
	}
}

func TestListenAndServe(t *testing.T) {
	s, img := newServer(t)
	addrCh := make(chan string, 1)
	s.AddrCh = addrCh
	s.Out = io.Discard
	s.Err = io.Discard

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- s.ListenAndServe(ctx, "127.0.0.1:0") }()

	// Wait for the server to bind and send us its address.
	var addr string
	select {
	case addr = <-addrCh:
	case <-time.After(2 * time.Second):
		cancel()
		t.Fatal("server did not start in time")
	}

	// Upload a real file via the JSON API.
	body, _ := json.Marshal(map[string]any{"list": []string{img}})
	resp, err := http.Post("http://"+addr+"/upload", "application/json", bytes.NewReader(body))
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	defer resp.Body.Close()
	var pr picgoResponse
	json.NewDecoder(resp.Body).Decode(&pr)
	if !pr.Success || len(pr.Result) == 0 {
		cancel()
		t.Fatalf("upload failed: %+v", pr)
	}

	cancel()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("server error on shutdown: %v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("server did not shut down in time")
	}
}
