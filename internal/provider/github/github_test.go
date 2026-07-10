package githubprovider

import (
	"context"
	"encoding/json"
	"fmt"
	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
)

func ghfile(t *testing.T) string {
	f := t.TempDir() + "/a.png"
	os.WriteFile(f, []byte("png"), 0600)
	return f
}
func ghrequest(t *testing.T, remote string) model.UploadRequest {
	p := ghfile(t)
	f, err := os.Open(p)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { f.Close() })
	st, _ := f.Stat()
	return model.UploadRequest{LocalPath: p, FileName: "a.png", Body: f, Size: st.Size(), RemotePath: remote, ContentType: "image/png"}
}
func TestCreateAndOverwrite(t *testing.T) {
	var exists bool
	var gotSHA bool
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.Contains(r.Header.Get("Authorization"), "secret") {
		} else {
			t.Error("auth missing")
		}
		if r.Method == "GET" {
			if exists {
				fmt.Fprint(w, `{"sha":"abc"}`)
			} else {
				w.WriteHeader(404)
			}
			return
		}
		var v map[string]any
		json.NewDecoder(r.Body).Decode(&v)
		_, gotSHA = v["sha"]
		w.WriteHeader(201)
		fmt.Fprint(w, "{}")
	}))
	defer s.Close()
	p := NewWithAPI("gh", config.ProviderConfig{Owner: "o", Repo: "r", Token: "secret", Branch: "main"}, s.Client(), s.URL)
	_, e := p.Upload(context.Background(), ghrequest(t, "a b.png"))
	if e != nil {
		t.Fatal(e)
	}
	exists = true
	req := ghrequest(t, "a.png")
	req.Overwrite = true
	_, e = p.Upload(context.Background(), req)
	if e != nil || !gotSHA {
		t.Fatalf("overwrite failed: %v sha=%v", e, gotSHA)
	}
}
func TestExistsRateLimitAndTokenSafety(t *testing.T) {
	mode := 0
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if mode == 0 {
			fmt.Fprint(w, `{"sha":"x"}`)
			return
		}
		w.Header().Set("X-RateLimit-Remaining", "0")
		w.WriteHeader(403)
		fmt.Fprint(w, "secret")
	}))
	defer s.Close()
	p := NewWithAPI("gh", config.ProviderConfig{Owner: "o", Repo: "r", Token: "secret"}, s.Client(), s.URL)
	_, e := p.Upload(context.Background(), ghrequest(t, "a.png"))
	if e == nil {
		t.Fatal("expected exists")
	}
	mode = 1
	_, e = p.Upload(context.Background(), ghrequest(t, "a.png"))
	if e == nil || strings.Contains(e.Error(), "secret") {
		t.Fatalf("unsafe rate error: %v", e)
	}
}
