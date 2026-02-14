package rust_language

import (
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"

	"github.com/bazelbuild/bazel-gazelle/language"
	"github.com/bazelbuild/bazel-gazelle/rule"

	messages "coppice/tools/gazelle_rust/proto"
)

// Metadata about a generated rule for use during resolution.
type RuleData struct {
	Responses []*messages.ParseResponse
}

func (l *rustLang) GenerateRules(args language.GenerateArgs) language.GenerateResult {
	result := language.GenerateResult{}

	dirName := path.Base(args.Rel)
	if args.Rel == "" {
		dirName = path.Base(args.Config.RepoRoot)
	}

	filesInExistingRules := make(map[string]bool)
	existingRuleNames := make(map[string]bool)

	// Process existing rules: clone them, filter deleted files, and collect
	// imports.
	if args.File != nil {
		for _, existingRule := range args.File.Rules {
			existingRuleNames[existingRule.Name()] = true

			kind := existingRule.Kind()
			if kind != "rust_library" && kind != "rust_binary" && kind != "rust_test" {
				continue
			}

			var validSrcs []string

			// Re-discover sources to pick up new files.
			if kind == "rust_library" && fileExists(args.Dir, "lib.rs") {
				validSrcs = l.discoverModules(args.Dir, "lib.rs")
			} else if kind == "rust_test" {
				validSrcs = l.collectTestFiles(args.Dir, filesInExistingRules)
			} else {
				for _, filename := range existingRule.AttrStrings("srcs") {
					if fileExists(args.Dir, filename) {
						validSrcs = append(validSrcs, filename)
					}
				}
			}

			for _, src := range validSrcs {
				filesInExistingRules[src] = true
			}

			l.cloneExistingRule(&result, kind, existingRule.Name(), args.Dir, validSrcs)
		}
	}

	// Collect candidate crate roots from the current directory.
	// Module files and test files in subdirectories are discovered separately.
	var crateRootCandidates []string
	for _, filename := range args.RegularFiles {
		if strings.HasSuffix(filename, ".rs") && !strings.Contains(filename, "/") {
			crateRootCandidates = append(crateRootCandidates, filename)
		}
	}

	if len(crateRootCandidates) == 0 {
		return result
	}

	claimedFiles := make(map[string]bool)
	for f := range filesInExistingRules {
		claimedFiles[f] = true
	}

	// lib.rs -> rust_library
	if fileExists(args.Dir, "lib.rs") && !filesInExistingRules["lib.rs"] && !existingRuleNames[dirName] {
		srcs := l.discoverModules(args.Dir, "lib.rs")
		for _, src := range srcs {
			claimedFiles[src] = true
		}
		l.emitNewRule(&result, "rust_library", dirName, args.Dir, srcs)
	}

	// Files with `fn main()` -> rust_binary
	for _, filename := range crateRootCandidates {
		if claimedFiles[filename] || strings.HasSuffix(filename, "_test.rs") {
			continue
		}

		fullPath := path.Join(args.Dir, filename)
		response, err := l.parser.Parse(fullPath)
		if err != nil || !response.Success || !response.HasMain {
			continue
		}

		targetName := strings.TrimSuffix(filename, ".rs")
		if existingRuleNames[targetName] {
			continue
		}

		l.emitNewRule(&result, "rust_binary", targetName, args.Dir, []string{filename})
		claimedFiles[filename] = true
	}

	// `*_test.rs` files -> rust_test
	testRuleName := dirName + "_test"
	if !existingRuleNames[testRuleName] {
		testFiles := l.collectTestFiles(args.Dir, claimedFiles)
		if len(testFiles) > 0 {
			l.emitNewRule(&result, "rust_test", testRuleName, args.Dir, testFiles)
		}
	}

	return result
}

func (l *rustLang) emitNewRule(result *language.GenerateResult, kind, name, dir string, srcs []string) {
	r := rule.NewRule(kind, name)
	r.SetAttr("srcs", srcs)
	if kind == "rust_library" {
		r.SetAttr("visibility", []string{"//:__subpackages__"})
	}
	result.Gen = append(result.Gen, r)
	result.Imports = append(result.Imports, RuleData{Responses: l.parseSrcs(dir, srcs)})
}

func (l *rustLang) cloneExistingRule(result *language.GenerateResult, kind, name, dir string, srcs []string) {
	r := rule.NewRule(kind, name)
	r.SetAttr("srcs", srcs)
	result.Gen = append(result.Gen, r)
	result.Imports = append(result.Imports, RuleData{Responses: l.parseSrcs(dir, srcs)})
}

func (l *rustLang) parseSrcs(dir string, srcs []string) []*messages.ParseResponse {
	var responses []*messages.ParseResponse
	for _, src := range srcs {
		if !strings.HasSuffix(src, ".rs") {
			continue
		}
		response, err := l.parser.Parse(path.Join(dir, src))
		if err == nil && response.Success {
			responses = append(responses, response)
		}
	}
	return responses
}

// Recursively discovers all source files for a crate starting from a root file.
func (l *rustLang) discoverModules(dir, rootFile string) []string {
	srcs := []string{rootFile}
	visited := make(map[string]bool)
	visited[rootFile] = true

	l.discoverModulesRecursive(dir, rootFile, &srcs, visited)

	sort.Strings(srcs)
	return srcs
}

func (l *rustLang) discoverModulesRecursive(dir, file string, srcs *[]string, visited map[string]bool) {
	fullPath := filepath.Join(dir, file)
	response, err := l.parser.Parse(fullPath)
	if err != nil {
		return
	}

	fileDir := filepath.Dir(file)
	if fileDir == "." {
		fileDir = ""
	}

	for _, modName := range response.ExternalModules {
		// Try adjacent file: {mod}.rs
		adjacentFile := filepath.Join(fileDir, modName+".rs")
		if !visited[adjacentFile] && fileExists(dir, adjacentFile) {
			visited[adjacentFile] = true
			*srcs = append(*srcs, adjacentFile)
			l.discoverModulesRecursive(dir, adjacentFile, srcs, visited)
			continue
		}

		// Try subdir with mod.rs: {mod}/mod.rs
		modFile := filepath.Join(fileDir, modName, "mod.rs")
		if !visited[modFile] && fileExists(dir, modFile) {
			visited[modFile] = true
			*srcs = append(*srcs, modFile)
			l.discoverModulesRecursive(dir, modFile, srcs, visited)
		}
	}
}

// Find all `*_test.rs` files in the directory and subdirectories, stopping at
// package boundaries (directories with BUILD files).
func (l *rustLang) collectTestFiles(dir string, claimedFiles map[string]bool) []string {
	var testFiles []string

	filepath.Walk(dir, func(p string, info os.FileInfo, err error) error {
		if err != nil {
			return nil
		}

		if info.IsDir() {
			if p == dir {
				return nil
			}
			if isPackageDir(p) {
				return filepath.SkipDir
			}
			return nil
		}

		relPath, err := filepath.Rel(dir, p)
		if err != nil {
			return nil
		}

		if !claimedFiles[relPath] && strings.HasSuffix(relPath, "_test.rs") {
			testFiles = append(testFiles, relPath)
		}

		return nil
	})

	sort.Strings(testFiles)
	return testFiles
}

// Check if a directory is a Bazel package.
func isPackageDir(dir string) bool {
	for _, buildFile := range []string{"BUILD", "BUILD.bazel", "MODULE.bazel"} {
		if _, err := os.Stat(filepath.Join(dir, buildFile)); err == nil {
			return true
		}
	}
	return false
}

func fileExists(dir, file string) bool {
	info, err := os.Stat(filepath.Join(dir, file))
	return err == nil && !info.IsDir()
}
