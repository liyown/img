package provider

import (
	"context"
	"fmt"

	"github.com/liyown/img/internal/config"
	"github.com/liyown/img/internal/model"
	githubprovider "github.com/liyown/img/internal/provider/github"
	httpprovider "github.com/liyown/img/internal/provider/http"
	s3provider "github.com/liyown/img/internal/provider/s3"
)

func New(ctx context.Context, name string, c config.ProviderConfig) (model.Provider, error) {
	resolved, err := config.ResolveProvider(c)
	if err != nil {
		return nil, fmt.Errorf("resolve credentials for provider %q: %w", name, err)
	}
	c = resolved
	switch c.Type {
	case "http":
		return httpprovider.New(name, c, nil), nil
	case "s3":
		return s3provider.New(ctx, name, c)
	case "github":
		return githubprovider.New(name, c, nil), nil
	default:
		return nil, fmt.Errorf("unknown provider type %q", c.Type)
	}
}
