class Mycelia < Formula
  desc "Local, content-agnostic knowledge index for agent retrieval"
  homepage "https://github.com/dnlbox/mycelia"
  url "https://github.com/dnlbox/mycelia/archive/refs/tags/v0.1.4.tar.gz"
  sha256 "281ce4c89c121207f0bb2eef60da475c18bd4b5e219db7ec89de44d3da279d51"
  license "Apache-2.0"

  depends_on "rust" => :build
  depends_on "onnxruntime"

  def install
    system "cargo", "install",
      *std_cargo_args(path: "crates/mycelia-cli"),
      "--no-default-features",
      "--features", "semantic-system-ort"
  end

  test do
    (testpath/"corpus").mkpath
    (testpath/"corpus/notes.txt").write("alpha beta answer\n")

    system bin/"mycelia", "index",
      testpath/"corpus",
      "--database",
      testpath/"index.sqlite3"

    output = shell_output("#{bin}/mycelia find 'alpha answer' " \
                          "--database #{testpath}/index.sqlite3 --strategy fts5-reranked")

    assert_match "notes.txt", output
  end
end
