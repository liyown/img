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

func TestInspectBytesDetectsPNG(t *testing.T) {
	data := []byte("\x89PNG\r\n\x1a\nbody-bytes")
	typ, size, err := InspectBytes(data, "photo.png", 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	if typ != "image/png" {
		t.Fatalf("type=%s", typ)
	}
	if size != int64(len(data)) {
		t.Fatalf("size=%d want %d", size, len(data))
	}
}

func TestInspectBytesRejectsNonImage(t *testing.T) {
	if _, _, err := InspectBytes([]byte("just plain text"), "x.png", 1<<20); err == nil {
		t.Fatal("plain text accepted as image")
	}
}

func TestInspectBytesEnforcesSize(t *testing.T) {
	if _, _, err := InspectBytes([]byte("\x89PNG\r\n\x1a\nxxxx"), "x.png", 4); err == nil {
		t.Fatal("oversized data accepted")
	}
	if _, _, err := InspectBytes(nil, "x.png", 100); err == nil {
		t.Fatal("empty data accepted")
	}
}
