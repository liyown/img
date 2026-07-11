package mdimg

import (
	"reflect"
	"testing"
)

func srcs(refs []Ref) []string {
	out := make([]string, len(refs))
	for i, r := range refs {
		out[i] = r.Src
	}
	return out
}

func TestExtractMarkdown(t *testing.T) {
	doc := `# Title

![a local](./images/a.png)
![remote](https://cdn.example.com/b.jpg "a title")
![alt](<path with spaces.png>)
Some prose mentioning (a.png) that is not an image.
`
	got := srcs(Extract(doc))
	want := []string{"./images/a.png", "https://cdn.example.com/b.jpg", "path with spaces.png"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v want %v", got, want)
	}
}

func TestExtractHTML(t *testing.T) {
	doc := `<img src="https://x.test/a.png" alt="x">
<img alt='y' src='local/b.gif'>
<IMG SRC="c.webp">`
	got := srcs(Extract(doc))
	want := []string{"https://x.test/a.png", "local/b.gif", "c.webp"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v want %v", got, want)
	}
}

func TestExtractSkipsNonReplaceable(t *testing.T) {
	doc := `![data](data:image/png;base64,iVBOR)
![anchor](#section)
![proto](//cdn.test/x.png)
![ftp](ftp://host/x.png)
![empty]()
![ok](real.png)`
	got := srcs(Extract(doc))
	want := []string{"real.png"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v want %v", got, want)
	}
}

func TestExtractDeduplicates(t *testing.T) {
	doc := "![](a.png) then again ![](a.png) and ![](b.png)"
	refs := Extract(doc)
	if len(refs) != 2 {
		t.Fatalf("expected 2 distinct refs, got %d: %v", len(refs), srcs(refs))
	}
	if refs[0].Src != "a.png" || refs[0].occurrences != 2 {
		t.Fatalf("dedup/count wrong: %+v", refs[0])
	}
}

func TestRewriteMarkdownPreservesAltAndTitle(t *testing.T) {
	doc := `![my alt](./a.png "the title") and ![](./a.png)`
	out := Rewrite(doc, map[string]string{"./a.png": "https://cdn/a.png"})
	want := `![my alt](https://cdn/a.png "the title") and ![](https://cdn/a.png)`
	if out != want {
		t.Fatalf("got %q want %q", out, want)
	}
}

func TestRewriteHTML(t *testing.T) {
	doc := `<img src="local.png" alt="x">`
	out := Rewrite(doc, map[string]string{"local.png": "https://cdn/local.png"})
	want := `<img src="https://cdn/local.png" alt="x">`
	if out != want {
		t.Fatalf("got %q want %q", out, want)
	}
}

func TestRewriteDoesNotTouchProse(t *testing.T) {
	// "a.png" appears in prose without image delimiters — must not be replaced.
	doc := "The file a.png-backup is separate. ![](a.png)"
	out := Rewrite(doc, map[string]string{"a.png": "https://cdn/a.png"})
	want := "The file a.png-backup is separate. ![](https://cdn/a.png)"
	if out != want {
		t.Fatalf("got %q want %q", out, want)
	}
}

func TestRewriteSkipsEmptyOrIdentity(t *testing.T) {
	doc := "![](a.png) ![](b.png)"
	out := Rewrite(doc, map[string]string{"a.png": "", "b.png": "b.png"})
	if out != doc {
		t.Fatalf("empty/identity replacements should be no-ops, got %q", out)
	}
}
