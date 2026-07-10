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
