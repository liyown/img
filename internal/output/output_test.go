package output

import (
	"bytes"
	"encoding/json"
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
	if got := FormatURL("https://x.test/a)\nINJECT", "markdown"); got != "![](https://x.test/a\\)INJECT)" {
		t.Fatalf("markdown=%q", got)
	}
}
