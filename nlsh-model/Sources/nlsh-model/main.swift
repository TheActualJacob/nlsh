import Foundation
import FoundationModels

import _Concurrency

struct NlshModel {
    static func main() async {
        let args = CommandLine.arguments

        // ── Availability check mode ──────────────────────────────────────────
        if args.contains("--check") {
            let model = SystemLanguageModel.default
            switch model.availability {
            case .available:
                print("available")
                exit(0)
            case .unavailable(.deviceNotEligible):
                print("unavailable:deviceNotEligible")
                exit(1)
            case .unavailable(.appleIntelligenceNotEnabled):
                print("unavailable:appleIntelligenceNotEnabled")
                exit(1)
            case .unavailable(.modelNotReady):
                print("unavailable:modelNotReady")
                exit(1)
            case .unavailable(let other):
                print("unavailable:\(other)")
                exit(1)
            }
        }

        // ── Inference mode ───────────────────────────────────────────────────
        guard case .available = SystemLanguageModel.default.availability else {
            fputs("nlsh-model: model not available\n", stderr)
            exit(1)
        }

        // Read full prompt from stdin.
        var promptText = ""
        while let line = readLine(strippingNewline: false) {
            promptText += line
        }
        guard !promptText.isEmpty else {
            fputs("nlsh-model: empty prompt\n", stderr)
            exit(2)
        }

        let session = LanguageModelSession()
        let stream = session.streamResponse(to: Prompt(promptText))

        var previous = ""
        do {
            for try await snapshot in stream {
                // ResponseStream<String> yields cumulative snapshots via snapshot.content.
                // Emit only the new suffix since the last snapshot.
                let current = snapshot.content
                let delta = String(current.dropFirst(previous.count))
                if !delta.isEmpty {
                    print(delta, terminator: "")
                    fflush(stdout)
                }
                previous = current
            }
        } catch {
            fputs("nlsh-model: generation error: \(error)\n", stderr)
            exit(2)
        }

        print() // final newline
        exit(0)
    }
}

await NlshModel.main()
