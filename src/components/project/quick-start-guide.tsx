import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"

export function QuickStartGuide({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return <Dialog open={open} onOpenChange={onOpenChange}>
    <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
      <DialogHeader><DialogTitle>Your first wiki, in a minute</DialogTitle><DialogDescription>Start small. You can explore the rest as you go.</DialogDescription></DialogHeader>
      <ol className="space-y-4 text-sm leading-relaxed">
        <li><strong>1. Create a wiki.</strong><p className="text-muted-foreground">Choose a name and a folder. Your wiki lives here as Markdown files.</p></li>
        <li><strong>2. Add something to learn from.</strong><p className="text-muted-foreground">Choose your AI model in Settings, then open Sources to import a document, notes, or a link.</p></li>
        <li><strong>3. Build and explore.</strong><p className="text-muted-foreground">Imported sources are queued to build linked pages. Read them in Wiki, find ideas with Search, or follow connections in Graph.</p></li>
        <li><strong>4. Make it yours.</strong><p className="text-muted-foreground">Edit pages and check Review for suggested changes. Connect your agent and optionally sync accepted pages in Knowledge → Library.</p></li>
      </ol>
      <p className="text-xs text-muted-foreground">Reopen this guide any time with the ? button.</p>
      <Button onClick={() => onOpenChange(false)}>Let me explore</Button>
    </DialogContent>
  </Dialog>
}
