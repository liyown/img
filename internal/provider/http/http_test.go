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
func request(t *testing.T) model.UploadRequest {
	p := image(t)
	f, err := os.Open(p)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { f.Close() })
	st, _ := f.Stat()
	return model.UploadRequest{LocalPath: p, FileName: "a.png", Body: f, Size: st.Size(), RemotePath: "a.png", ContentType: "image/png"}
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
	p := New("custom", config.ProviderConfig{URL: s.URL, FileField: "asset", URLJSONPath: "data.url", Headers: map[string]string{"X-Key": "value"}, Fields: map[string]string{"folder": "images"}, AllowInsecure: true}, nil)
	r, e := p.Upload(context.Background(), request(t))
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
		fmt.Fprint(w, "private-token"+strings.Repeat("x", 2000))
	}))
	defer s.Close()
	p := New("x", config.ProviderConfig{URL: s.URL, URLJSONPath: "url", Headers: map[string]string{"Authorization": secret}, AllowInsecure: true}, nil)
	_, e := p.Upload(context.Background(), request(t))
	if e == nil || strings.Contains(e.Error(), "private-token") || len(e.Error()) > 700 {
		t.Fatalf("unsafe error: %v", e)
	}
}

func TestRejectsInsecureEndpointAndUnsafeReturnedURL(t *testing.T) {
	p := New("x", config.ProviderConfig{URL: "http://example.test/upload", URLJSONPath: "url"}, nil)
	if err := p.Validate(context.Background()); err == nil {
		t.Fatal("insecure endpoint accepted")
	}
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { fmt.Fprint(w, `{"url":"javascript:alert(1)"}`) }))
	defer s.Close()
	p = New("x", config.ProviderConfig{URL: s.URL, URLJSONPath: "url", AllowInsecure: true}, s.Client())
	if _, err := p.Upload(context.Background(), request(t)); err == nil {
		t.Fatal("unsafe response URL accepted")
	}
}

func TestRefusesCrossOriginRedirectWithHeaders(t *testing.T) {
	reached := false
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { reached = true }))
	defer target.Close()
	source := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL, http.StatusTemporaryRedirect)
	}))
	defer source.Close()
	p := New("x", config.ProviderConfig{URL: source.URL, URLJSONPath: "url", AllowInsecure: true, Headers: map[string]string{"X-Api-Key": "secret"}}, source.Client())
	if err := p.Test(context.Background()); err == nil {
		t.Fatal("cross-origin redirect accepted")
	}
	if reached {
		t.Fatal("sensitive headers reached redirect target")
	}
}

func TestOversizedResponse(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, strings.Repeat("x", maxResponse+1))
	}))
	defer s.Close()
	p := New("x", config.ProviderConfig{URL: s.URL, URLJSONPath: "url", AllowInsecure: true}, nil)
	_, err := p.Upload(context.Background(), request(t))
	if err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("expected bounded response error: %v", err)
	}
}
func TestCancel(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { <-r.Context().Done() }))
	defer s.Close()
	p := New("x", config.ProviderConfig{URL: s.URL, URLJSONPath: "url", AllowInsecure: true}, &http.Client{Timeout: time.Second})
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, e := p.Upload(ctx, request(t))
	if e == nil {
		t.Fatal("expected cancellation")
	}
}
