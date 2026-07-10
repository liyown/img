package output

import (
	"encoding/json"
	"fmt"
	"io"
	"strings"

	"github.com/liyown/img/internal/upload"
)

func FormatURL(url, format string) string {
	switch format {
	case "markdown":
		return "![](" + url + ")"
	case "html":
		return `<img src="` + url + `" alt="">`
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
			fmt.Fprintf(w, "Error: %s: %s\n", r.LocalPath, r.Error)
		}
	}
	return nil
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
