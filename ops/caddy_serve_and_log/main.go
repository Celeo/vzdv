package vzdv_caddy_static_serve_and_log

import (
	"database/sql"
	"fmt"
	"net/http"

	"github.com/caddyserver/caddy/v2"
	"github.com/caddyserver/caddy/v2/caddyconfig/caddyfile"
	"github.com/caddyserver/caddy/v2/caddyconfig/httpcaddyfile"
	"github.com/caddyserver/caddy/v2/modules/caddyhttp"
	_ "github.com/mattn/go-sqlite3"
	"go.uber.org/zap"
)

func init() {
	caddy.RegisterModule(Middleware{})
	httpcaddyfile.RegisterHandlerDirective("serve_and_log", parseCaddyfile)
}

type Middleware struct {
	DbFile  string `json:"db_file"`
	DbTable string `json:"db_table"`

	logger *zap.Logger
	db     *sql.DB
}

func (Middleware) CaddyModule() caddy.ModuleInfo {
	return caddy.ModuleInfo{
		ID:  "http.handlers.serve_and_log",
		New: func() caddy.Module { return new(Middleware) },
	}
}

func (m *Middleware) Provision(ctx caddy.Context) error {
	m.logger = ctx.Logger()
	db, err := sql.Open("sqlite3", m.DbFile)
	if err != nil {
		return err
	}
	m.db = db
	return nil
}

func (m *Middleware) Validate() error {
	if m.db == nil {
		return fmt.Errorf("db pointer is nil")
	}
	return nil
}

func (m *Middleware) Cleanup() error {
	return m.db.Close()
}

func (m Middleware) ServeHTTP(w http.ResponseWriter, r *http.Request, next caddyhttp.Handler) error {
	cookie := r.Header.Get("cookie")
	if len(cookie) > 3 {
		cookie = cookie[3:]
		path := r.URL.Path[1:]
		m.logger.Sugar().Infof("Saw cookie '%s' access resource '%s'", cookie, path)

		statement := fmt.Sprintf("INSERT INTO %s VALUES (NULL, ?, ?)", m.DbTable)
		if _, err := m.db.Exec(statement, cookie, path); err != nil {
			return err
		}
	}

	return next.ServeHTTP(w, r)
}

func (m *Middleware) UnmarshalCaddyfile(d *caddyfile.Dispenser) error {
	d.Next()
	if !d.NextArg() {
		return d.ArgErr()
	}
	m.DbFile = d.Val()
	if !d.NextArg() {
		return d.ArgErr()
	}
	m.DbTable = d.Val()
	return nil
}

func parseCaddyfile(h httpcaddyfile.Helper) (caddyhttp.MiddlewareHandler, error) {
	var m Middleware
	err := m.UnmarshalCaddyfile(h.Dispenser)
	return m, err
}

var (
	_ caddy.Provisioner           = (*Middleware)(nil)
	_ caddy.Validator             = (*Middleware)(nil)
	_ caddyhttp.MiddlewareHandler = (*Middleware)(nil)
	_ caddyfile.Unmarshaler       = (*Middleware)(nil)
	_ caddy.CleanerUpper          = (*Middleware)(nil)
)
