package pathgen

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestGenerateAndHashStable(t *testing.T) {
	d := t.TempDir()
	f := filepath.Join(d, "截图 image.png")
	os.WriteFile(f, []byte("same"), 0600)
	now := time.Date(2026, 7, 10, 1, 2, 3, 0, time.UTC)
	a, e := Generate(f, `{year}\{month}\{filename}`, "blog/assets", "original", now)
	if e != nil {
		t.Fatal(e)
	}
	if a != "blog/assets/2026/07/截图 image.png" {
		t.Fatalf("got %q", a)
	}
	h1, _ := Generate(f, "{hash}.png", "", "hash", now)
	h2, _ := Generate(f, "{hash}.png", "", "hash", now)
	if h1 != h2 {
		t.Fatal("hash is unstable")
	}
	if EscapeURLPath(a) != "blog/assets/2026/07/%E6%88%AA%E5%9B%BE%20image.png" {
		t.Fatal(EscapeURLPath(a))
	}
}
func TestRejectTraversal(t *testing.T) {
	for _, p := range []string{"../x.png", "/x.png", "a//b"} {
		if _, e := Validate(p); e == nil {
			t.Fatalf("accepted %q", p)
		}
	}
}
