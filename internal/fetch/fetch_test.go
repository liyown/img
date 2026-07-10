package fetch

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestIsURL(t *testing.T) {
	cases := map[string]bool{
		"https://example.com/a.png": true,
		"http://example.com/a.png":  true,
		"a.png":                     false,
		"/tmp/a.png":                false,
		"ftp://example.com/a.png":   false,
	}
	for in, want := range cases {
		if IsURL(in) != want {
			t.Errorf("IsURL(%q) = %v, want %v", in, !want, want)
		}
	}
}

func TestFetchDownloads(t *testing.T) {
	png := []byte("\x89PNG\r\n\x1a\nhello")
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "image/png")
		w.Write(png)
	}))
	defer s.Close()
	// httptest is HTTP, so allowInsecure must be true and we use its client.
	res, err := Fetch(context.Background(), s.URL+"/photo.png", 1<<20, true, s.Client())
	if err != nil {
		t.Fatal(err)
	}
	if string(res.Data) != string(png) {
		t.Fatalf("data mismatch: %q", res.Data)
	}
	if res.FileName != "photo.png" {
		t.Fatalf("filename = %q", res.FileName)
	}
}

func TestFetchRejectsInsecureByDefault(t *testing.T) {
	_, err := Fetch(context.Background(), "http://example.com/a.png", 1<<20, false, nil)
	if err == nil || !strings.Contains(err.Error(), "HTTPS") {
		t.Fatalf("expected HTTPS rejection, got %v", err)
	}
}

func TestFetchRejectsOversized(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte(strings.Repeat("x", 100)))
	}))
	defer s.Close()
	_, err := Fetch(context.Background(), s.URL+"/big.png", 10, true, s.Client())
	if err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("expected size limit error, got %v", err)
	}
}

func TestFetchRejectsBadStatus(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Error(w, "nope", http.StatusNotFound)
	}))
	defer s.Close()
	_, err := Fetch(context.Background(), s.URL+"/missing.png", 1<<20, true, s.Client())
	if err == nil || !strings.Contains(err.Error(), "404") {
		t.Fatalf("expected 404 error, got %v", err)
	}
}

func TestFetchRefusesCrossOriginRedirect(t *testing.T) {
	other := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte("secret"))
	}))
	defer other.Close()
	src := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, other.URL+"/x.png", http.StatusFound)
	}))
	defer src.Close()
	_, err := Fetch(context.Background(), src.URL+"/a.png", 1<<20, true, src.Client())
	if err == nil || !strings.Contains(err.Error(), "cross-origin") {
		t.Fatalf("expected cross-origin redirect refusal, got %v", err)
	}
}

func TestFetchFileNameFromContentType(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "image/png")
		fmt.Fprint(w, "\x89PNG\r\n\x1a\n")
	}))
	defer s.Close()
	// URL path has no extension; name should come from Content-Type.
	res, err := Fetch(context.Background(), s.URL+"/download", 1<<20, true, s.Client())
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasSuffix(res.FileName, ".png") {
		t.Fatalf("expected .png extension from content-type, got %q", res.FileName)
	}
}
