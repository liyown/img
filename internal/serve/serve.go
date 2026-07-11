// Package serve runs a PicGo-compatible local HTTP server that accepts image
// uploads and re-hosts them via the configured img provider. This lets any
// editor or tool that supports a custom PicGo server (Typora, Obsidian,
// VS Code extensions, …) use img as its upload back-end.
//
// API (port 36677 by default, bound to 127.0.0.1):
//
//	POST /upload
//	  Content-Type: application/json
//	  {"list": ["/absolute/path/to/file.png", ...]}
//	  → {"success": true,  "result": ["https://cdn.../file.png"]}
//	  → {"success": false, "msg":    "error description"}
//
// Multipart form upload is also accepted (one or more "file" fields).
package serve

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
	"github.com/liyown/img/internal/upload"
)

// Options configures the upload behaviour for each incoming request.
type Options struct {
	UploadOpts upload.Options
}

// Server is a PicGo-compatible HTTP server.
type Server struct {
	Provider model.Provider
	Cfg      config.Upload
	Opts     Options
	Out      io.Writer // progress / log lines
	Err      io.Writer
	// AddrCh, if non-nil, receives "host:port" once the server is listening.
	// Useful in tests to discover the dynamically allocated port without
	// parsing log output (which would require a mutex).
	AddrCh chan<- string
}

// picgoRequest is the JSON body sent by Typora / Obsidian / PicGo clients.
type picgoRequest struct {
	List []string `json:"list"`
}

// picgoResponse is the JSON body returned to the client.
type picgoResponse struct {
	Success bool     `json:"success"`
	Result  []string `json:"result,omitempty"`
	Msg     string   `json:"msg,omitempty"`
}

// Handler returns an http.Handler for /upload.
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/upload", s.handleUpload)
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		http.Error(w, "img serve: use POST /upload", http.StatusNotFound)
	})
	return mux
}

func (s *Server) handleUpload(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		w.Header().Set("Allow", "POST")
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	var files []string
	var tempFiles []string
	defer func() {
		for _, f := range tempFiles {
			os.Remove(f)
		}
	}()

	ct := r.Header.Get("Content-Type")
	switch {
	case strings.HasPrefix(ct, "application/json"):
		var req picgoRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			s.jsonError(w, "invalid JSON body: "+err.Error())
			return
		}
		files = req.List

	default:
		// Multipart form — save each uploaded file to a temp file.
		if err := r.ParseMultipartForm(s.Cfg.MaxSize); err != nil {
			s.jsonError(w, "parse multipart: "+err.Error())
			return
		}
		for _, fhs := range r.MultipartForm.File {
			for _, fh := range fhs {
				mf, err := fh.Open()
				if err != nil {
					continue
				}
				tmp, err := os.CreateTemp("", "img-serve-*-"+fh.Filename)
				if err != nil {
					mf.Close()
					continue
				}
				io.Copy(tmp, mf)
				mf.Close()
				tmp.Close()
				tempFiles = append(tempFiles, tmp.Name())
				files = append(files, tmp.Name())
			}
		}
	}

	if len(files) == 0 {
		s.jsonError(w, "no files provided")
		return
	}

	start := time.Now()
	results := upload.Run(r.Context(), s.Provider, s.Cfg, files, s.Opts.UploadOpts)

	var urls []string
	var errs []string
	for _, res := range results {
		if res.Success {
			urls = append(urls, res.URL)
			fmt.Fprintf(s.Out, "[%s] ✓ %s → %s\n",
				time.Now().Format("15:04:05"), res.LocalPath, res.URL)
		} else {
			errs = append(errs, res.Error)
			fmt.Fprintf(s.Err, "[%s] ✗ %s: %s\n",
				time.Now().Format("15:04:05"), res.LocalPath, res.Error)
		}
	}

	_ = start
	w.Header().Set("Content-Type", "application/json")
	if len(urls) == 0 {
		json.NewEncoder(w).Encode(picgoResponse{
			Success: false,
			Msg:     strings.Join(errs, "; "),
		})
		return
	}
	json.NewEncoder(w).Encode(picgoResponse{
		Success: true,
		Result:  urls,
	})
}

func (s *Server) jsonError(w http.ResponseWriter, msg string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusBadRequest)
	json.NewEncoder(w).Encode(picgoResponse{Success: false, Msg: msg})
}

// ListenAndServe starts the server and blocks until ctx is cancelled.
func (s *Server) ListenAndServe(ctx context.Context, addr string) error {
	srv := &http.Server{
		Addr:    addr,
		Handler: s.Handler(),
	}

	// Verify the address is local-only when no explicit bind is set.
	host, _, err := net.SplitHostPort(addr)
	if err == nil && host != "127.0.0.1" && host != "::1" && host != "localhost" {
		fmt.Fprintf(s.Err,
			"Warning: serving on %s — consider binding to 127.0.0.1 for security\n", addr)
	}

	ln, err := net.Listen("tcp", addr)
	if err != nil {
		return fmt.Errorf("listen %s: %w", addr, err)
	}
	fmt.Fprintf(s.Out, "img serve listening on http://%s/upload\n", ln.Addr())
	fmt.Fprintf(s.Out, "Configure your editor's upload URL to: http://%s/upload\n\n", ln.Addr())
	if s.AddrCh != nil {
		s.AddrCh <- ln.Addr().String()
	}

	errCh := make(chan error, 1)
	go func() { errCh <- srv.Serve(ln) }()

	select {
	case <-ctx.Done():
		shutCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = srv.Shutdown(shutCtx)
		return nil
	case err := <-errCh:
		return err
	}
}
