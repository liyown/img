package output

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"

	"github.com/liyown/img/internal/upload"
)

func TestJSONOutputStable(t *testing.T) {
	results := []upload.FileResult{{LocalPath: "a.png", Success: true, RemotePath: "2026/07/a.png", URL: "https://x/a.png", Provider: "r2", Size: 10, ContentType: "image/png"}, {LocalPath: "b.png", Success: false, Error: "file not found"}}
	var b bytes.Buffer
	if err := Render(&b, "json", results); err != nil {
		t.Fatal(err)
	}
	var got struct {
		Success bool                `json:"success"`
		Files   []upload.FileResult `json:"files"`
	}
	if err := json.Unmarshal(b.Bytes(), &got); err != nil {
		t.Fatal(err)
	}
	if got.Success || len(got.Files) != 2 || !got.Files[0].Success || got.Files[1].Error != "file not found" {
		t.Fatalf("unexpected JSON: %s", b.String())
	}
}

func TestFormatEscapesHTMLAndMarkdown(t *testing.T) {
	if got := FormatURL(`https://x.test/a" onerror="alert(1)`, `html`); got == `<img src="https://x.test/a" onerror="alert(1)" alt="">` {
		t.Fatal("HTML URL was not escaped")
	}
	// Newline in URL must be stripped from both the safe URL and alt text.
	got := FormatURL("https://x.test/a)\nINJECT", "markdown")
	if strings.Contains(got, "\n") {
		t.Fatalf("newline survived sanitisation: %q", got)
	}
	if strings.Contains(got, ")(") {
		t.Fatalf("markdown link syntax broken: %q", got)
	}
}

func TestMarkdownAltText(t *testing.T) {
	got := FormatURL("https://cdn.example.com/2026/07/screenshot.png", "markdown")
	if got != "![screenshot.png](https://cdn.example.com/2026/07/screenshot.png)" {
		t.Fatalf("got %q", got)
	}
}

func TestClipboardTextJSON(t *testing.T) {
	results := []upload.FileResult{
		{LocalPath: "a.png", Success: true, URL: "https://x/a.png", Provider: "r2"},
		{LocalPath: "b.png", Success: false, Error: "fail"},
	}
	text := ClipboardText("json", results)
	var doc struct {
		Success bool `json:"success"`
	}
	if err := json.Unmarshal([]byte(text), &doc); err != nil {
		t.Fatalf("clipboard json is not valid JSON: %v\n%s", err, text)
	}
	if doc.Success {
		t.Fatal("partial failure should not report success=true")
	}
	if !strings.Contains(text, "https://x/a.png") {
		t.Fatal("clipboard json missing URL")
	}
}

func TestClipboardTextMarkdown(t *testing.T) {
	results := []upload.FileResult{
		{Success: true, URL: "https://cdn.test/img.png"},
		{Success: false, Error: "err"},
	}
	text := ClipboardText("markdown", results)
	if text != "![img.png](https://cdn.test/img.png)" {
		t.Fatalf("got %q", text)
	}
}

