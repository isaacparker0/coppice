package rust_language

import (
	"flag"

	"github.com/bazelbuild/bazel-gazelle/config"
	"github.com/bazelbuild/bazel-gazelle/label"
	"github.com/bazelbuild/bazel-gazelle/language"
	"github.com/bazelbuild/bazel-gazelle/rule"
)

const langName = "rust"

type rustLang struct {
	parser *Parser
}

func NewLanguage() language.Language {
	return &rustLang{
		parser: NewParser(),
	}
}

func (*rustLang) Name() string { return langName }

func (*rustLang) Kinds() map[string]rule.KindInfo {
	return map[string]rule.KindInfo{
		"rust_library": {
			NonEmptyAttrs:  map[string]bool{"srcs": true},
			MergeableAttrs: map[string]bool{"srcs": true, "deps": true},
			ResolveAttrs:   map[string]bool{"deps": true},
		},
		"rust_binary": {
			NonEmptyAttrs:  map[string]bool{"srcs": true},
			MergeableAttrs: map[string]bool{"srcs": true, "deps": true},
			ResolveAttrs:   map[string]bool{"deps": true},
		},
		"rust_test": {
			NonEmptyAttrs:  map[string]bool{"srcs": true},
			MergeableAttrs: map[string]bool{"srcs": true, "deps": true},
			ResolveAttrs:   map[string]bool{"deps": true},
		},
		// Index rust_prost_library so we can resolve deps to proto targets.
		"rust_prost_library": {
			MergeableAttrs: map[string]bool{},
			ResolveAttrs:   map[string]bool{},
		},
	}
}

func (*rustLang) Loads() []rule.LoadInfo {
	return []rule.LoadInfo{
		{
			Name:    "//tools/bazel/macros:rust.bzl",
			Symbols: []string{"rust_library", "rust_binary", "rust_test"},
		},
	}
}

func (*rustLang) RegisterFlags(fs *flag.FlagSet, cmd string, c *config.Config) {}

func (*rustLang) CheckFlags(fs *flag.FlagSet, c *config.Config) error { return nil }

func (*rustLang) KnownDirectives() []string { return nil }

func (*rustLang) Configure(c *config.Config, rel string, f *rule.File) {}

func (*rustLang) Embeds(r *rule.Rule, from label.Label) []label.Label { return nil }

func (*rustLang) Fix(c *config.Config, f *rule.File) {}
