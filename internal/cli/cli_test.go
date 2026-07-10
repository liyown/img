package cli

import (
	"bytes"
	"context"
	"errors"
	"fmt"
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
	if strings.TrimSpace(out.String()) != "![](https://cdn.test/image.png)" {
		t.Fatalf("output=%q", out.String())
	}
	if !strings.Contains(stderr.String(), "clipboard copy failed") {
		t.Fatalf("missing warning: %s", stderr.String())
	}
}
