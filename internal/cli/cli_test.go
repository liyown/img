package cli

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/liyown/img/internal/config"
)

type badClipboard struct{}

func (badClipboard) Write(string) error { return errors.New("clipboard unavailable") }

func TestShorthandUploadEndToEnd(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseMultipartForm(1 << 20); err != nil {
			t.Error(err)
		}
		if _, _, err := r.FormFile("file"); err != nil {
			t.Error(err)
		}
		fmt.Fprint(w, `{"data":{"url":"https://cdn.test/image.png"}}`)
	}))
	defer server.Close()
	d := t.TempDir()
	cfg := config.Defaults()
	cfg.DefaultProvider = "local"
	cfg.Providers["local"] = config.ProviderConfig{Type: "http", URL: server.URL, URLJSONPath: "data.url", AllowInsecure: true}
	cfgPath := filepath.Join(d, "config.toml")
	if err := config.Save(cfgPath, cfg); err != nil {
		t.Fatal(err)
	}
	image := filepath.Join(d, "image.png")
	if err := os.WriteFile(image, []byte("\x89PNG\r\n\x1a\nbody"), 0600); err != nil {
		t.Fatal(err)
	}
	var out, stderr bytes.Buffer
	c := &CLI{Out: &out, Err: &stderr, In: strings.NewReader(""), Clipboard: badClipboard{}, GlobalPath: cfgPath}
	code := c.Run(context.Background(), []string{image, "--format", "markdown", "--copy"})
	if code != 0 {
		t.Fatalf("code=%d stderr=%s", code, stderr.String())
	}
	// Alt text is now the filename extracted from the URL path.
	if strings.TrimSpace(out.String()) != "![image.png](https://cdn.test/image.png)" {
		t.Fatalf("output=%q", out.String())
	}
	if !strings.Contains(stderr.String(), "clipboard copy failed") {
		t.Fatalf("missing warning: %s", stderr.String())
	}
}

func TestInitGitHub(t *testing.T) {
	d := t.TempDir()
	cfgPath := filepath.Join(d, "config.toml")
	var out bytes.Buffer
	c := &CLI{Out: &out, Err: io.Discard, In: strings.NewReader(""), GlobalPath: cfgPath}
	code := c.Run(context.Background(), []string{"init",
		"--type", "github",
		"--name", "gh",
		"--owner", "myorg",
		"--repo", "images",
		"--token", "${IMG_GITHUB_TOKEN}",
	})
	if code != 0 {
		t.Fatalf("code=%d", code)
	}
	cfg, err := config.Load(cfgPath, "")
	if err != nil {
		t.Fatal(err)
	}
	p, ok := cfg.Providers["gh"]
	if !ok || p.Type != "github" || p.Owner != "myorg" || p.Repo != "images" || p.Token != "${IMG_GITHUB_TOKEN}" {
		t.Fatalf("unexpected provider config: %+v", p)
	}
	if cfg.DefaultProvider != "gh" {
		t.Fatalf("default provider not set: %s", cfg.DefaultProvider)
	}
	// Should emit the 1MB advisory for GitHub providers.
	if !strings.Contains(out.String(), "1 MB") {
		t.Fatalf("missing GitHub size advisory: %s", out.String())
	}
}

func TestProviderListSorted(t *testing.T) {
	d := t.TempDir()
	cfgPath := filepath.Join(d, "config.toml")
	cfg := config.Defaults()
	cfg.DefaultProvider = "r2"
	cfg.Providers["r2"] = config.ProviderConfig{Type: "s3", Bucket: "b", PublicURL: "https://cdn.test", AccessKey: "${K}", SecretKey: "${S}"}
	cfg.Providers["aliyun"] = config.ProviderConfig{Type: "s3", Bucket: "b2", PublicURL: "https://cdn2.test", AccessKey: "${K2}", SecretKey: "${S2}"}
	if err := config.Save(cfgPath, cfg); err != nil {
		t.Fatal(err)
	}
	var out bytes.Buffer
	c := &CLI{Out: &out, Err: io.Discard, In: strings.NewReader(""), GlobalPath: cfgPath}
	if code := c.Run(context.Background(), []string{"provider", "list"}); code != 0 {
		t.Fatalf("code=%d", code)
	}
	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	// lines[0] = header, lines[1] = aliyun (alphabetically first), lines[2] = r2
	if len(lines) < 3 {
		t.Fatalf("expected at least 3 lines, got: %s", out.String())
	}
	if !strings.HasPrefix(lines[1], "aliyun") {
		t.Fatalf("expected aliyun first, got: %q", lines[1])
	}
	if !strings.HasPrefix(lines[2], "r2") {
		t.Fatalf("expected r2 second, got: %q", lines[2])
	}
}

func TestQuietFlagSuppressesOutput(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		fmt.Fprint(w, `{"data":{"url":"https://cdn.test/image.png"}}`)
	}))
	defer server.Close()
	d := t.TempDir()
	cfg := config.Defaults()
	cfg.DefaultProvider = "local"
	cfg.Providers["local"] = config.ProviderConfig{Type: "http", URL: server.URL, URLJSONPath: "data.url", AllowInsecure: true}
	cfgPath := filepath.Join(d, "config.toml")
	config.Save(cfgPath, cfg)
	image := filepath.Join(d, "image.png")
	os.WriteFile(image, []byte("\x89PNG\r\n\x1a\nbody"), 0600)
	var out bytes.Buffer
	c := &CLI{Out: &out, Err: io.Discard, In: strings.NewReader(""), GlobalPath: cfgPath}
	code := c.Run(context.Background(), []string{image, "--quiet"})
	if code != 0 {
		t.Fatalf("code=%d", code)
	}
	if out.Len() != 0 {
		t.Fatalf("quiet mode produced output: %q", out.String())
	}
}

// newHTTPProvider sets up a test server that accepts a multipart upload and
// returns a predictable CDN URL, then writes a config pointing at it.
func newHTTPProvider(t *testing.T, cdnURL string) (cfgPath string, server *httptest.Server) {
	t.Helper()
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		r.ParseMultipartForm(4 << 20)
		fmt.Fprintf(w, `{"data":{"url":%q}}`, cdnURL)
	}))
	t.Cleanup(server.Close)
	d := t.TempDir()
	cfg := config.Defaults()
	cfg.DefaultProvider = "local"
	cfg.Providers["local"] = config.ProviderConfig{
		Type: "http", URL: server.URL, URLJSONPath: "data.url", AllowInsecure: true,
	}
	cfgPath = filepath.Join(d, "config.toml")
	if err := config.Save(cfgPath, cfg); err != nil {
		t.Fatal(err)
	}
	return cfgPath, server
}

func TestRewriteInPlace(t *testing.T) {
	cfgPath, _ := newHTTPProvider(t, "https://cdn.test/a.png")
	d := t.TempDir()

	// Write a local image next to the article.
	img := filepath.Join(d, "a.png")
	if err := os.WriteFile(img, []byte("\x89PNG\r\n\x1a\nbody"), 0600); err != nil {
		t.Fatal(err)
	}
	article := filepath.Join(d, "post.md")
	original := "# Title\n\n![alt](a.png)\n\nParagraph.\n"
	if err := os.WriteFile(article, []byte(original), 0600); err != nil {
		t.Fatal(err)
	}

	var stderr bytes.Buffer
	c := &CLI{Out: io.Discard, Err: &stderr, In: strings.NewReader(""), GlobalPath: cfgPath}
	code := c.Run(context.Background(), []string{"rewrite", article})
	if code != 0 {
		t.Fatalf("code=%d stderr=%s", code, stderr.String())
	}

	// File should have been rewritten in place.
	got, err := os.ReadFile(article)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(got), "https://cdn.test/a.png") {
		t.Fatalf("image URL not replaced: %s", got)
	}
	if strings.Contains(string(got), "](a.png)") {
		t.Fatal("original local reference still present after rewrite")
	}
	// Alt text and surrounding prose must be preserved.
	if !strings.Contains(string(got), "![alt](") {
		t.Fatalf("alt text lost: %s", got)
	}
	if !strings.Contains(string(got), "# Title") || !strings.Contains(string(got), "Paragraph.") {
		t.Fatalf("prose lost: %s", got)
	}
}

func TestRewriteStdin(t *testing.T) {
	cfgPath, _ := newHTTPProvider(t, "https://cdn.test/remote.jpg")
	d := t.TempDir()
	// remote URL image in the article — fetching requires AllowInsecure because
	// httptest is HTTP. We use a real PNG header in the fake response to pass
	// filetype detection.
	imgSrc := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "image/png")
		w.Write([]byte("\x89PNG\r\n\x1a\nbody"))
	}))
	defer imgSrc.Close()

	article := "![](a.png)\n![remote](" + imgSrc.URL + "/remote.png)\n"
	// Write a local image so the local ref is uploadable.
	if err := os.WriteFile(filepath.Join(d, "a.png"), []byte("\x89PNG\r\n\x1a\nbody"), 0600); err != nil {
		t.Fatal(err)
	}
	_ = d // articleDir resolves to cwd; local ref "a.png" won't resolve correctly
	// Focus this test on stdin→stdout routing, not on local image resolution.
	article = "![remote](" + imgSrc.URL + "/remote.png)\n"

	var out, stderr bytes.Buffer
	c := &CLI{Out: &out, Err: &stderr, In: strings.NewReader(article), GlobalPath: cfgPath}
	code := c.Run(context.Background(), []string{"rewrite", "--allow-insecure"})
	if code != 0 {
		t.Fatalf("code=%d stderr=%s", code, stderr.String())
	}
	// Result goes to stdout.
	if !strings.Contains(out.String(), "https://cdn.test/") {
		t.Fatalf("image URL not replaced in stdout output: %q", out.String())
	}
}

func TestRewriteStdout(t *testing.T) {
	cfgPath, _ := newHTTPProvider(t, "https://cdn.test/a.png")
	d := t.TempDir()
	img := filepath.Join(d, "a.png")
	os.WriteFile(img, []byte("\x89PNG\r\n\x1a\nbody"), 0600)
	article := filepath.Join(d, "post.md")
	os.WriteFile(article, []byte("![](a.png)\n"), 0600)
	originalContent, _ := os.ReadFile(article)

	var out bytes.Buffer
	c := &CLI{Out: &out, Err: io.Discard, In: strings.NewReader(""), GlobalPath: cfgPath}
	code := c.Run(context.Background(), []string{"rewrite", article, "--stdout"})
	if code != 0 {
		t.Fatalf("code=%d", code)
	}
	// --stdout: result goes to stdout, original file untouched.
	if !strings.Contains(out.String(), "https://cdn.test/a.png") {
		t.Fatalf("expected rewritten content in stdout, got: %q", out.String())
	}
	afterContent, _ := os.ReadFile(article)
	if string(afterContent) != string(originalContent) {
		t.Fatal("--stdout must not modify the original file")
	}
}

func TestRewriteNoImages(t *testing.T) {
	cfgPath, _ := newHTTPProvider(t, "https://cdn.test/x.png")
	d := t.TempDir()
	article := filepath.Join(d, "post.md")
	original := "# No images here\n\nJust text.\n"
	os.WriteFile(article, []byte(original), 0600)

	c := &CLI{Out: io.Discard, Err: io.Discard, In: strings.NewReader(""), GlobalPath: cfgPath}
	code := c.Run(context.Background(), []string{"rewrite", article})
	if code != 0 {
		t.Fatalf("code=%d", code)
	}
	// File must be unchanged.
	got, _ := os.ReadFile(article)
	if string(got) != original {
		t.Fatalf("file changed when no images: %q", got)
	}
}
