// Package fetch downloads remote images so they can be re-hosted on a
// configured provider ("URL rehosting"). Its security posture matches the
// HTTP provider: HTTPS is required (HTTP only when explicitly allowed),
// redirects must stay same-origin and are capped, and the response body is
// bounded. It does NOT perform IP-level SSRF filtering — private, loopback,
// and cloud-metadata addresses are reachable, same as the HTTP provider.
package fetch

import (
	"context"
	"fmt"
	"io"
	"mime"
	"net/http"
	"net/url"
	"path"
	"strings"
	"time"
)

const defaultTimeout = 60 * time.Second

// IsURL reports whether s looks like an http(s) URL that Fetch can handle.
func IsURL(s string) bool {
	return strings.HasPrefix(s, "http://") || strings.HasPrefix(s, "https://")
}

// Result holds a downloaded image.
type Result struct {
	Data     []byte
	FileName string // best-effort name derived from the URL / headers
	URL      string // the original request URL
}

// Fetch downloads rawURL and returns its bytes. It reads at most maxSize
// bytes; a larger response is rejected. allowInsecure permits plain HTTP.
// client may be nil, in which case a default client is used (useful for tests).
func Fetch(ctx context.Context, rawURL string, maxSize int64, allowInsecure bool, client *http.Client) (Result, error) {
	u, err := secureURL(rawURL, allowInsecure)
	if err != nil {
		return Result{}, fmt.Errorf("fetch %s: %w", rawURL, err)
	}

	if client == nil {
		client = &http.Client{Timeout: defaultTimeout}
	}
	secured := *client
	origin := u
	originalRedirect := client.CheckRedirect
	secured.CheckRedirect = func(req *http.Request, via []*http.Request) error {
		if len(via) >= 5 {
			return fmt.Errorf("too many HTTP redirects")
		}
		if req.URL.Scheme != origin.Scheme || !strings.EqualFold(req.URL.Host, origin.Host) {
			return fmt.Errorf("refusing cross-origin redirect from %s to %s", origin.Host, req.URL.Host)
		}
		if originalRedirect != nil {
			return originalRedirect(req, via)
		}
		return nil
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, rawURL, nil)
	if err != nil {
		return Result{}, fmt.Errorf("build request for %s: %w", rawURL, err)
	}
	resp, err := secured.Do(req)
	if err != nil {
		return Result{}, fmt.Errorf("download %s: %w", rawURL, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return Result{}, fmt.Errorf("download %s failed with status %d", rawURL, resp.StatusCode)
	}

	// Read one byte past the limit so we can detect an oversized body.
	data, err := io.ReadAll(io.LimitReader(resp.Body, maxSize+1))
	if err != nil {
		return Result{}, fmt.Errorf("read %s: %w", rawURL, err)
	}
	if int64(len(data)) > maxSize {
		return Result{}, fmt.Errorf("download %s exceeds maximum size of %d bytes", rawURL, maxSize)
	}
	if len(data) == 0 {
		return Result{}, fmt.Errorf("download %s returned an empty body", rawURL)
	}

	return Result{Data: data, FileName: fileName(u, resp.Header.Get("Content-Type")), URL: rawURL}, nil
}

// fileName derives a reasonable download name from the URL path, falling back
// to the Content-Type's extension, then to "download".
func fileName(u *url.URL, contentType string) string {
	base := path.Base(u.Path)
	if base != "" && base != "." && base != "/" && strings.Contains(base, ".") {
		return base
	}
	// No usable extension in the path; try the Content-Type.
	if ct := strings.SplitN(contentType, ";", 2)[0]; ct != "" {
		if exts, _ := mime.ExtensionsByType(strings.TrimSpace(ct)); len(exts) > 0 {
			stem := base
			if stem == "" || stem == "." || stem == "/" {
				stem = "download"
			}
			return stem + exts[0]
		}
	}
	if base == "" || base == "." || base == "/" {
		return "download"
	}
	return base
}

// secureURL mirrors the HTTP provider's URL hygiene: absolute, no userinfo or
// fragment, HTTPS unless HTTP is explicitly allowed, and no control characters.
func secureURL(raw string, allowInsecure bool) (*url.URL, error) {
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" {
		return nil, fmt.Errorf("URL must be absolute")
	}
	if u.User != nil || u.Fragment != "" {
		return nil, fmt.Errorf("URL must not contain user information or a fragment")
	}
	if u.Scheme != "https" && !(allowInsecure && u.Scheme == "http") {
		return nil, fmt.Errorf("URL must use HTTPS (pass --allow-insecure for trusted HTTP sources)")
	}
	for _, r := range raw {
		if r < 32 || r == 127 {
			return nil, fmt.Errorf("URL contains a control character")
		}
	}
	return u, nil
}
