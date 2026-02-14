package rust_language

import (
	"sort"
	"strings"

	"github.com/bazelbuild/bazel-gazelle/config"
	"github.com/bazelbuild/bazel-gazelle/label"
	"github.com/bazelbuild/bazel-gazelle/repo"
	"github.com/bazelbuild/bazel-gazelle/resolve"
	"github.com/bazelbuild/bazel-gazelle/rule"
)

// Rust standard library crates that don't need external dependencies.
// Note: Primitive types (u32, char, etc.) are filtered by the parser.
var builtins = map[string]bool{
	"std":        true,
	"core":       true,
	"alloc":      true,
	"proc_macro": true,
	"test":       true,
}

// Crates provided by external rules.
var providedCrates = map[string]string{
	"prost":    "@rules_rust_prost//private/3rdparty/crates:prost",
	"runfiles": "@rules_rust//tools/runfiles",
}

const cratesPrefix = "@crates//:"

// Return the crate name for a rule based on its package path.
func getCrateName(r *rule.Rule, pkg string) string {
	if r.Kind() == "rust_library" {
		// Our wrapper macro converts package paths to crate names using double
		// underscores.
		return strings.ReplaceAll(pkg, "/", "__")
	}
	return r.Name()
}

func (l *rustLang) Imports(c *config.Config, r *rule.Rule, f *rule.File) []resolve.ImportSpec {
	pkg := ""
	if f != nil {
		pkg = f.Pkg
	}

	var crateName string
	switch r.Kind() {
	case "rust_library":
		crateName = getCrateName(r, pkg)
	case "rust_prost_library":
		// rust_prost_library derives crate name from its proto attribute.
		protoAttr := r.AttrString("proto")
		if protoAttr == "" {
			return nil
		}
		protoLabel, err := label.Parse(protoAttr)
		if err != nil {
			return nil
		}
		crateName = strings.ReplaceAll(protoLabel.Name, "-", "_")
	default:
		return nil
	}

	return []resolve.ImportSpec{
		{
			Lang: langName,
			Imp:  crateName,
		},
	}
}

func (l *rustLang) Resolve(c *config.Config, ix *resolve.RuleIndex, rc *repo.RemoteCache, r *rule.Rule, imports any, from label.Label) {
	ruleData, ok := imports.(RuleData)
	if !ok {
		return
	}

	deps := make(map[string]bool)

	// Get this rule's crate name to skip self-imports.
	selfCrateName := getCrateName(r, from.Pkg)

	for _, response := range ruleData.Responses {
		for _, importName := range response.Imports {
			if builtins[importName] {
				continue
			}

			if importName == selfCrateName {
				continue
			}

			normalizedImport := strings.ReplaceAll(importName, "-", "_")

			spec := resolve.ImportSpec{
				Lang: langName,
				Imp:  normalizedImport,
			}

			// Check workspace first via rule index.
			if matches := ix.FindRulesByImportWithConfig(c, spec, langName); len(matches) > 0 {
				depLabel := matches[0].Label
				relativeLabel := depLabel.Rel(from.Repo, from.Pkg)
				deps[relativeLabel.String()] = true
				continue
			}

			if providedLabel, ok := providedCrates[normalizedImport]; ok {
				deps[providedLabel] = true
				continue
			}

			crateName := getExternalCrates(c).GetName(normalizedImport)
			deps[cratesPrefix+crateName] = true
		}
	}

	if len(deps) > 0 {
		r.SetAttr("deps", sortedKeys(deps))
	} else {
		r.DelAttr("deps")
	}
}

func sortedKeys(set map[string]bool) []string {
	keys := make([]string, 0, len(set))
	for key := range set {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}
