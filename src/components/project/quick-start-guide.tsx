import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"

export function QuickStartGuide({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return <Dialog open={open} onOpenChange={onOpenChange}>
    <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
      <DialogHeader><DialogTitle>Your first wiki, in a minute</DialogTitle><DialogDescription>Start small. You can explore the rest as you go.</DialogDescription></DialogHeader>
      <ol className="space-y-4 text-sm leading-relaxed">
        <li><strong>1. Create a wiki.</strong><p className="text-muted-foreground">Choose a name and a folder. Your wiki lives here as Markdown files.</p></li>
        <li><strong>2. Connect your coding agent.</strong><p className="text-muted-foreground">Open Connectors and choose Athenaeum Agent, Claude Code, or Cursor. Keep using your existing agent and account; this connection needs no extra model API key.</p></li>
        <li><strong>3. Work, review, and explore.</strong><p className="text-muted-foreground">As you work, your agent quietly recalls useful knowledge and proposes new findings. Open Review to accept them, read pages in Wiki, or follow connections in Graph.</p></li>
        <li><strong>4. Connect Athenaeum Cloud.</strong><p className="text-muted-foreground">In Knowledge → Library, link your wiki and sync accepted pages across computers. You choose what to share. You can also clone an existing wiki from the welcome screen.</p></li>
      </ol>
      <p className="text-xs text-muted-foreground">Reopen this guide any time with the ? button.</p>
      <Button onClick={() => onOpenChange(false)}>Let me explore</Button>
    </DialogContent>
  </Dialog>
}
