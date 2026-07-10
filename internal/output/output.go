package output

import (
	"encoding/json"
	"fmt"
	"html"
	"io"
	"strings"

	"github.com/liyown/img/internal/upload"
)

func FormatURL(url, format string) string {
	switch format {
	case "markdown":
		safe := strings.NewReplacer("\\", "\\\\", ")", "\\)", "\r", "", "\n", "").Replace(url)
		return "![](" + safe + ")"
	case "html":
		return `<img src="` + html.EscapeString(url) + `" alt="">`
	default:
		return url
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
func ClipboardText(format string, results []upload.FileResult) string {
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
