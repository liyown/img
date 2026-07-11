// Package mdimg extracts image references from Markdown (and inline HTML
// <img> tags) and rewrites them in place. It uses regular expressions rather
// than a full Markdown parser: this keeps the dependency footprint at zero and
// covers the overwhelming majority of real-world articles.
package mdimg

import (
	"regexp"
	"strings"
)

// Ref is a single image reference discovered in a document.
type Ref struct {
	// Src is the raw image source exactly as it appears in the document
	// (a local path or an http(s) URL). This is what gets replaced.
	Src string
	// occurrences counts how many times Src appears across all matches; used
	// only for reporting.
	occurrences int
}

var (
	// Markdown image: ![alt](src "optional title")
	// Group 1 = the src (up to the first whitespace or closing paren).
	mdImage = regexp.MustCompile(`!\[[^\]]*\]\(\s*(<[^>]*>|[^)\s]+)`)
	// HTML <img ... src="..." ...> with single or double quotes.
	htmlImage = regexp.MustCompile(`(?i)<img\b[^>]*?\bsrc\s*=\s*("([^"]*)"|'([^']*)')`)
)

// Extract returns the distinct image sources referenced by doc, in first-seen
// order. data: URIs and empty/anchor references are skipped.
func Extract(doc string) []Ref {
	seen := map[string]int{}
	var order []string

	add := func(src string) {
		src = strings.TrimSpace(src)
		// Markdown allows the source to be wrapped in angle brackets: <path>.
		if strings.HasPrefix(src, "<") && strings.HasSuffix(src, ">") {
			src = strings.TrimSpace(src[1 : len(src)-1])
		}
		if !isReplaceable(src) {
			return
		}
		if _, ok := seen[src]; !ok {
			order = append(order, src)
		}
		seen[src]++
	}

	for _, m := range mdImage.FindAllStringSubmatch(doc, -1) {
		add(m[1])
	}
	for _, m := range htmlImage.FindAllStringSubmatch(doc, -1) {
		// m[2] = double-quoted content, m[3] = single-quoted content.
		if m[2] != "" || strings.Contains(m[1], `""`) {
			add(m[2])
		} else {
			add(m[3])
		}
	}

	refs := make([]Ref, 0, len(order))
	for _, s := range order {
		refs = append(refs, Ref{Src: s, occurrences: seen[s]})
	}
	return refs
}

// isReplaceable reports whether a source should be uploaded and rewritten.
// It skips data: URIs, protocol-relative and non-http schemes we can't fetch,
// pure anchors, and empty strings. Local paths and http(s) URLs pass.
func isReplaceable(src string) bool {
	if src == "" {
		return false
	}
	if strings.HasPrefix(src, "#") {
		return false
	}
	lower := strings.ToLower(src)
	switch {
	case strings.HasPrefix(lower, "data:"):
		return false
	case strings.HasPrefix(lower, "//"):
		// Protocol-relative — fetch can't resolve the scheme.
		return false
	}
	// Reject any explicit scheme other than http/https (e.g. ftp:, mailto:,
	// file:). A Windows path like C:\x is not a URL scheme we care about, but
	// "scheme:" detection here only trips on "word:" with no backslash, which
	// local relative paths never contain.
	if i := strings.Index(src, ":"); i > 0 {
		scheme := lower[:i]
		if isScheme(scheme) && scheme != "http" && scheme != "https" {
			return false
		}
	}
	return true
}

// isScheme reports whether s is a plausible URI scheme (letters/digits/+-.).
func isScheme(s string) bool {
	if s == "" {
		return false
	}
	for i, r := range s {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z':
		case (r >= '0' && r <= '9' || r == '+' || r == '-' || r == '.') && i > 0:
		default:
			return false
		}
	}
	return true
}

// Rewrite replaces every occurrence of each mapping key (an original image
// source) with its mapped value (the new URL) in doc. Replacement is
// occurrence-based on the exact source token, so alt text and titles are
// preserved. Sources not present in the map are left untouched.
func Rewrite(doc string, replacements map[string]string) string {
	if len(replacements) == 0 {
		return doc
	}
	// Replace the longest sources first so that a source which is a prefix of
	// another (rare, but possible) does not corrupt the longer one.
	keys := make([]string, 0, len(replacements))
	for k, v := range replacements {
		if v == "" || k == v {
			continue
		}
		keys = append(keys, k)
	}
	sortByLenDesc(keys)

	for _, k := range keys {
		doc = replaceSrc(doc, k, replacements[k])
	}
	return doc
}

// replaceSrc replaces occurrences of the image source `from` with `to`, but
// only where `from` sits inside an image reference delimiter — i.e. preceded
// by "](" / "(" for Markdown or a src-quote for HTML. To stay simple and
// robust we replace the exact token `from` wherever it appears as a whole
// source: bounded on the left by one of ( " ' < whitespace and on the right by
// ) " ' > whitespace. This avoids touching unrelated prose that merely
// contains the same substring.
func replaceSrc(doc, from, to string) string {
	var b strings.Builder
	b.Grow(len(doc))
	for {
		i := strings.Index(doc, from)
		if i < 0 {
			b.WriteString(doc)
			break
		}
		leftOK := i == 0 || isBoundary(doc[i-1])
		rightIdx := i + len(from)
		rightOK := rightIdx >= len(doc) || isBoundary(doc[rightIdx])
		b.WriteString(doc[:i])
		if leftOK && rightOK {
			b.WriteString(to)
		} else {
			b.WriteString(from)
		}
		doc = doc[rightIdx:]
	}
	return b.String()
}

func isBoundary(c byte) bool {
	switch c {
	case '(', ')', '"', '\'', '<', '>', ' ', '\t', '\n', '\r':
		return true
	}
	return false
}

func sortByLenDesc(s []string) {
	for i := 1; i < len(s); i++ {
		for j := i; j > 0 && len(s[j]) > len(s[j-1]); j-- {
			s[j], s[j-1] = s[j-1], s[j]
		}
	}
}
