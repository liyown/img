package filetype

import (
	"os"
	"path/filepath"
	"testing"
)

func TestRejectsExtensionSpoofing(t *testing.T) {
	p := filepath.Join(t.TempDir(), "not-an-image.png")
	if err := os.WriteFile(p, []byte("plain text"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err := Inspect(p, 1<<20); err == nil {
		t.Fatal("plain text with .png extension was accepted")
	}
}
func TestDetectsSVGFromContent(t *testing.T) {
	p := filepath.Join(t.TempDir(), "image.bin")
	if err := os.WriteFile(p, []byte(`<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"></svg>`), 0600); err != nil {
		t.Fatal(err)
	}
	typ, _, err := Inspect(p, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	if typ != "image/svg+xml" {
		t.Fatalf("type=%s", typ)
	}
}
