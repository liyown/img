package output

import (
	"encoding/json"
	"fmt"
	"html"
	"io"
	"net/url"
	"path"
	"strings"

	"github.com/liyown/img/internal/upload"
)

func FormatURL(rawURL, format string) string {
	switch format {
	case "markdown":
		safe := strings.NewReplacer("\\", "\\\\", ")", "\\)", "\r", "", "\n", "").Replace(rawURL)
		// Extract filename from URL path for accessibility alt text.
		// Strip query/fragment first so they don't pollute the filename.
		alt := ""
		if u, err := url.Parse(rawURL); err == nil {
			seg := path.Base(u.Path)
			if seg != "." && seg != "/" {
				// Sanitize: strip characters that would break Markdown alt syntax.
				alt = strings.NewReplacer("\r", "", "\n", "", "[", "", "]", "").Replace(seg)
			}
		}
		return "![" + alt + "](" + safe + ")"
	case "html":
		return `<img src="` + html.EscapeString(rawURL) + `" alt="">`
	default:
		return rawURL
	}
}

func Render(w io.Writer, format string, results []upload.FileResult) error {
	if format == "json" {
		enc := json.NewEncoder(w)
		enc.SetIndent("", "  ")
		return enc.Encode(struct {
			Success bool                `json:"success"`
			Files   []upload.FileResult `json:"files"`
		}{all(results), results})
	}
	for _, r := range results {
		if r.Success {
			fmt.Fprintln(w, FormatURL(r.URL, format))
		} else {
			fmt.Fprintf(w, "Error: %s: %s\n", cleanTerminal(r.LocalPath), cleanTerminal(r.Error))
		}
	}
	return nil
}

func cleanTerminal(s string) string {
	return strings.Map(func(r rune) rune {
		if r < 32 && r != '\t' {
			return -1
		}
		if r == 127 {
			return -1
		}
		return r
	}, s)
}

// ClipboardText returns the text that should be written to the clipboard.
// For the json format it mirrors the full JSON document produced by Render,
// so that --format json --copy gives the same output in both destinations.
func ClipboardText(format string, results []upload.FileResult) string {
	if format == "json" {
		b, err := json.MarshalIndent(struct {
			Success bool                `json:"success"`
			Files   []upload.FileResult `json:"files"`
		}{all(results), results}, "", "  ")
		if err != nil {
			return ""
		}
		return string(b)
	}
	var x []string
	for _, r := range results {
		if r.Success {
			x = append(x, FormatURL(r.URL, format))
		}
	}
	return strings.Join(x, "\n")
}

func all(r []upload.FileResult) bool {
	if len(r) == 0 {
		return false
	}
	for _, x := range r {
		if !x.Success {
			return false
		}
	}
	return true
}
