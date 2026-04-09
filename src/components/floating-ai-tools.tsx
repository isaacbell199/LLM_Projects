'use client'

import { useState, useCallback } from 'react'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import {
  ArrowRight,
  Swords,
  Zap,
  Clapperboard,
  Undo2,
  RefreshCw,
  Square,
  Loader2,
  Sparkles
} from 'lucide-react'
import { motion, AnimatePresence } from 'framer-motion'
import { toast } from 'sonner'
import {
  tauriGenerateText,
  stopGeneration,
  onGenerationChunk,
  isTauri
} from '@/lib/tauri-api'

interface FloatingAIToolsProps {
  isModelLoaded: boolean
  editorContent: string
  lastGeneratedContent: string | null
  lastGeneratedIndex: number | null
  onApplySuggestion: (content: string) => void
  onStreamChunk: (chunk: string) => void
  onGenerationStart: () => void
  onGenerationEnd: () => void
  onClearLastGeneration: () => void
  onRegenerate: () => void
  isGenerating: boolean
  temperature: number
  maxTokens: number
  outputLanguage: string
  // Creative Settings (from Wizard or Editor)
  creativeCategory?: string
  creativeTone?: string
  creativeStyle?: string
  creativeTheme?: string
}

interface Suggestion {
  id: string
  text: string
}

// Prompts to generate dynamic suggestions based on story context
const SUGGESTION_GENERATION_PROMPTS: Record<string, string> = {
  next: `You are a creative writing assistant analyzing the current story. Read the story context carefully and generate 4 creative, intelligent suggestions for what could happen next.

Consider:
- Current plot trajectory and pacing
- Character motivations and relationships
- Unresolved tension or mysteries
- Natural story progression

Each suggestion should be brief (3-6 words), compelling, and feel organic to the story. Avoid generic ideas - make them specific to the current context.

Format: exactly 4 numbered suggestions, one per line.
Example format:
1. [specific suggestion based on story context]
2. [another contextual suggestion]
...`,

  conflict: `You are a creative writing assistant analyzing the current story. Generate 4 compelling conflict ideas that could emerge naturally from the current situation.

Consider:
- Character flaws and opposing goals
- External threats or obstacles
- Internal struggles and moral dilemmas
- Power dynamics and betrayals

Each suggestion should be brief (3-6 words), dramatic, and arise naturally from the established story. Make the conflict feel inevitable yet surprising.

Format: exactly 4 numbered suggestions, one per line.`,

  action: `You are a creative writing assistant analyzing the current story. Generate 4 dynamic action scene ideas that would feel thrilling and appropriate.

Consider:
- Physical stakes and dangers
- Chase, combat, or escape scenarios
- Time pressure and urgency
- Environmental hazards or advantages

Each suggestion should be brief (3-6 words), exciting, and fit naturally within the story's tone and setting.

Format: exactly 4 numbered suggestions, one per line.`,

  scenario: `You are a creative writing assistant analyzing the current story. Generate 4 interesting scenario developments that could take the story in compelling new directions.

Consider:
- Unexpected twists that recontextualize events
- New locations or world-building opportunities
- Surprising character revelations
- Thematic resonances and symbolism

Each suggestion should be brief (3-6 words), intriguing, and open new narrative possibilities while staying true to the story's spirit.

Format: exactly 4 numbered suggestions, one per line.`
}

// Prompts to generate a paragraph based on chosen suggestion
const PARAGRAPH_GENERATION_PROMPTS: Record<string, (suggestion: string) => string> = {
  next: (suggestion) => `Continue this story by incorporating: "${suggestion}".

Write ONE engaging paragraph (3-5 sentences) that:
- Flows naturally from the current text
- Maintains the story's established tone and voice
- Advances the plot meaningfully
- Shows rather than tells
- Uses specific, vivid details

Do not repeat or summarize previous content. Continue forward.`,

  conflict: (suggestion) => `Introduce this conflict into the story: "${suggestion}".

Write ONE dramatic paragraph (3-5 sentences) that:
- Creates genuine tension and stakes
- Reveals character through reaction
- Advances the plot organically
- Uses sensory details and strong verbs
- Makes the conflict feel inevitable yet surprising

Do not repeat previous content.`,

  action: (suggestion) => `Write an action scene based on: "${suggestion}".

Write ONE dynamic paragraph (3-5 sentences) that:
- Uses short, punchy sentences for impact
- Incorporates strong verbs and sensory details
- Creates urgency and momentum
- Shows physical movement and reaction
- Maintains clarity amidst chaos

Focus on the moment-to-moment action.`,

  scenario: (suggestion) => `Develop this scenario: "${suggestion}".

Write ONE atmospheric paragraph (3-5 sentences) that:
- Establishes the new direction smoothly
- Uses vivid, evocative language
- Creates intrigue and curiosity
- Maintains narrative coherence
- Grounds the reader in the new situation

Transition naturally from what came before.`
}

// Generate demo suggestions for browser mode (hoisted function)
function generateDemoSuggestions(toolId: string, context: string): Suggestion[] {
  const baseSuggestions: Record<string, string[][]> = {
    next: [
      ['A mysterious figure emerges', 'Hidden truth comes to light', 'Unexpected ally arrives', 'Time runs short'],
      ['Silent warning appears', 'Past catches up', 'New path reveals itself', 'Trust is questioned'],
      ['Storm approaches quickly', 'Old secret resurfaces', 'Journey takes a turn', 'Hope flickers dim']
    ],
    conflict: [
      ['Loyalty is tested', 'Hidden betrayal revealed', 'Rivalry ignites', 'Trust crumbles away'],
      ['Promises are broken', 'Power struggle begins', 'Old wounds reopen', 'Alliances fracture'],
      ['Secret exposed publicly', 'Loved ones threatened', 'Principles challenged', 'Loyalties divided']
    ],
    action: [
      ['Sudden chase begins', 'Fight for survival', 'Daring escape attempt', 'Race against time'],
      ['Ambush from shadows', 'Explosive confrontation', 'Narrow escape made', 'Battle erupts'],
      ['Pursuit through danger', 'Combat intensifies', 'Dramatic rescue', 'High-stakes standoff']
    ],
    scenario: [
      ['Journey to unknown', 'Ancient secret discovered', 'Unexpected reunion', 'Mysterious stranger'],
      ['Portal opens nearby', 'Hidden world revealed', 'Past returns', 'Future glimpsed'],
      ['Strange occurrence', 'Reality shifts subtly', 'Mystery deepens', 'Truth emerges']
    ]
  }

  const options = baseSuggestions[toolId] || baseSuggestions.next
  const randomIndex = Math.floor(Math.random() * options.length)
  return options[randomIndex].map((text, i) => ({ id: `${i}`, text }))
}

export function FloatingAITools({
  isModelLoaded,
  editorContent,
  lastGeneratedContent,
  onApplySuggestion,
  onStreamChunk,
  onGenerationStart,
  onGenerationEnd,
  onClearLastGeneration,
  onRegenerate,
  isGenerating,
  temperature,
  maxTokens,
  outputLanguage,
  // Creative Settings
  creativeCategory,
  creativeTone,
  creativeStyle,
  creativeTheme,
}: FloatingAIToolsProps) {
  const [activeTool, setActiveTool] = useState<string | null>(null)
  const [suggestions, setSuggestions] = useState<Suggestion[]>([])
  const [isGeneratingSuggestions, setIsGeneratingSuggestions] = useState(false)

  // Toggle tool and generate suggestions
  const handleToolClick = useCallback(async (toolId: string) => {
    if (activeTool === toolId) {
      setActiveTool(null) // Close if already open
      setSuggestions([])
      return
    }

    if (!isModelLoaded) {
      toast.error('Please load an AI model first')
      return
    }

    setActiveTool(toolId)
    setSuggestions([])
    setIsGeneratingSuggestions(true)

    try {
      if (isTauri()) {
        // Generate dynamic suggestions using AI
        let generatedText = ''
        
        const unsubscribe = onGenerationChunk((chunk) => {
          if (chunk.done) {
            // Parse the generated text into suggestions
            const lines = generatedText.split('\n').filter(line => line.trim())
            const parsedSuggestions: Suggestion[] = []
            
            lines.forEach((line, index) => {
              // Remove numbering (1., 2., 3., 4.) and clean up
              const cleaned = line.replace(/^\d+[\.\)]\s*/, '').trim()
              if (cleaned && parsedSuggestions.length < 4) {
                parsedSuggestions.push({
                  id: `suggestion-${index}`,
                  text: cleaned
                })
              }
            })

            // If we didn't get enough suggestions, add placeholders
            while (parsedSuggestions.length < 4) {
              parsedSuggestions.push({
                id: `suggestion-${parsedSuggestions.length}`,
                text: `Idea ${parsedSuggestions.length + 1}`
              })
            }

            setSuggestions(parsedSuggestions.slice(0, 4))
            setIsGeneratingSuggestions(false)
          } else {
            generatedText += chunk.content
          }
        })

        const storyContext = editorContent.slice(-3000)
        const prompt = SUGGESTION_GENERATION_PROMPTS[toolId]

        await tauriGenerateText({
          mode: 'suggestion',
          text: storyContext,
          context: prompt,
          temperature: 0.9, // Higher temperature for more creative suggestions
          maxTokens: 150,
          language: outputLanguage,
          selectedTone: creativeTone,
          customStyleName: creativeStyle,
          customGenreName: creativeCategory,
          customStyleInstruction: creativeTheme,
        })

        setTimeout(() => {
          unsubscribe()
        }, 30000)
      } else {
        // Demo mode - simulate dynamic suggestions based on context
        await new Promise(r => setTimeout(r, 1000))
        
        const contextWords = editorContent.toLowerCase().slice(-500)
        const demoSuggestions = generateDemoSuggestions(toolId, contextWords)
        setSuggestions(demoSuggestions)
        setIsGeneratingSuggestions(false)
      }
    } catch (error) {
      console.error('Failed to generate suggestions:', error)
      toast.error('Failed to generate suggestions')
      setIsGeneratingSuggestions(false)
      // Fallback suggestions
      setSuggestions([
        { id: '1', text: 'An unexpected event occurs' },
        { id: '2', text: 'A new character appears' },
        { id: '3', text: 'A secret is revealed' },
        { id: '4', text: 'The situation intensifies' }
      ])
    }
  }, [activeTool, isModelLoaded, editorContent, outputLanguage, creativeCategory, creativeTone, creativeStyle, creativeTheme])

  // Generate paragraph based on selected suggestion
  const handleSelectSuggestion = useCallback(async (suggestion: Suggestion) => {
    if (!isModelLoaded) {
      toast.error('Please load an AI model first')
      return
    }

    const toolId = activeTool
    if (!toolId) return

    setActiveTool(null) // Close suggestions panel
    setSuggestions([])
    onGenerationStart() // Notify that generation is starting

    try {
      if (isTauri()) {
        // Stream generation directly to editor
        const unsubscribe = onGenerationChunk((chunk) => {
          if (chunk.done) {
            onGenerationEnd() // Notify that generation ended
            toast.success('Content generated!')
          } else {
            onStreamChunk(chunk.content)
          }
        })

        const prompt = PARAGRAPH_GENERATION_PROMPTS[toolId](suggestion.text)

        await tauriGenerateText({
          mode: 'suggestion',
          text: editorContent.slice(-2000),
          context: prompt,
          temperature,
          maxTokens: 500,
          language: outputLanguage,
          selectedTone: creativeTone,
          customStyleName: creativeStyle,
          customGenreName: creativeCategory,
          customStyleInstruction: creativeTheme,
        })

        setTimeout(() => {
          unsubscribe()
        }, 60000)
      } else {
        // Demo mode - simulate generation
        const demoContent = ` ${suggestion.text}. The moment unfolded with unexpected intensity, each detail sharpening into focus as the situation evolved. What had seemed like a simple path forward now revealed layers of complexity that would change everything.`
        
        // Simulate streaming
        for (let i = 0; i < demoContent.length; i += 3) {
          await new Promise(r => setTimeout(r, 20))
          onStreamChunk(demoContent.slice(i, i + 3))
        }
        onGenerationEnd()
        toast.success('Content generated!')
      }
    } catch (error) {
      console.error('Generation error:', error)
      onGenerationEnd()
      toast.error('Failed to generate content')
    }
  }, [activeTool, isModelLoaded, editorContent, temperature, outputLanguage, onStreamChunk, onGenerationStart, onGenerationEnd, creativeCategory, creativeTone, creativeStyle, creativeTheme])

  // Stop generation
  const handleStopGeneration = useCallback(() => {
    stopGeneration()
    onGenerationEnd()
    toast.info('Generation stopped')
  }, [onGenerationEnd])

  const tools = [
    { 
      id: 'next', 
      icon: ArrowRight, 
      label: 'Next',
      color: 'text-sky-500',
      bg: 'hover:bg-sky-500/10',
      activeBg: 'bg-sky-500/20'
    },
    { 
      id: 'conflict', 
      icon: Swords, 
      label: 'Conflict',
      color: 'text-rose-500',
      bg: 'hover:bg-rose-500/10',
      activeBg: 'bg-rose-500/20'
    },
    { 
      id: 'action', 
      icon: Zap, 
      label: 'Action',
      color: 'text-amber-500',
      bg: 'hover:bg-amber-500/10',
      activeBg: 'bg-amber-500/20'
    },
    { 
      id: 'scenario', 
      icon: Clapperboard, 
      label: 'Scenario',
      color: 'text-violet-500',
      bg: 'hover:bg-violet-500/10',
      activeBg: 'bg-violet-500/20'
    },
  ]

  const actionTools = [
    { 
      id: 'clear', 
      icon: Undo2, 
      label: 'Clear',
      color: 'text-muted-foreground',
      bg: 'hover:bg-rose-500/10 hover:text-rose-500',
      disabled: !lastGeneratedContent,
      onClick: onClearLastGeneration
    },
    { 
      id: 'regenerate', 
      icon: RefreshCw, 
      label: 'Regenerate',
      color: 'text-muted-foreground',
      bg: 'hover:bg-emerald-500/10 hover:text-emerald-500',
      disabled: !lastGeneratedContent || isGenerating,
      onClick: onRegenerate
    },
  ]

  return (
    <div className="flex items-start gap-2">
      {/* Main Tools Column */}
      <div className="flex flex-col gap-2 items-center">
        {/* Suggestion Tools */}
        {tools.map((tool) => (
          <Button
            key={tool.id}
            variant="ghost"
            size="icon"
            className={cn(
              "h-10 w-10 rounded-xl transition-all relative group",
              tool.color,
              tool.bg,
              activeTool === tool.id && tool.activeBg
            )}
            disabled={!isModelLoaded || isGenerating || isGeneratingSuggestions}
            onClick={() => handleToolClick(tool.id)}
            title={tool.label}
          >
            <tool.icon className="h-5 w-5" />
            
            {/* Floating label on hover */}
            <div className={cn(
              "absolute right-full mr-2 px-2 py-1 rounded-lg bg-popover border border-border/50 text-xs font-medium whitespace-nowrap opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none shadow-md z-50",
              "text-foreground"
            )}>
              {tool.label}
            </div>
          </Button>
        ))}

        {/* Divider */}
        <div className="w-6 h-px bg-border/50 my-1" />

        {/* Action Tools */}
        {actionTools.map((tool) => (
          <Button
            key={tool.id}
            variant="ghost"
            size="icon"
            className={cn(
              "h-10 w-10 rounded-xl transition-all relative group",
              tool.color,
              tool.bg,
              tool.disabled && "opacity-40 cursor-not-allowed"
            )}
            disabled={tool.disabled}
            onClick={tool.onClick}
            title={tool.label}
          >
            <tool.icon className={cn("h-5 w-5", isGenerating && tool.id === 'regenerate' && "animate-spin")} />
            
            {/* Floating label */}
            <div className={cn(
              "absolute right-full mr-2 px-2 py-1 rounded-lg bg-popover border border-border/50 text-xs font-medium whitespace-nowrap opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none shadow-md z-50",
              "text-foreground"
            )}>
              {tool.label}
            </div>
          </Button>
        ))}
      </div>

      {/* Suggestions Panel - Appears on the RIGHT */}
      <AnimatePresence>
        {activeTool && (
          <motion.div
            initial={{ opacity: 0, x: -10, scale: 0.95 }}
            animate={{ opacity: 1, x: 0, scale: 1 }}
            exit={{ opacity: 0, x: -10, scale: 0.95 }}
            transition={{ duration: 0.15 }}
            className="flex flex-col gap-1.5 p-2 rounded-xl bg-popover/95 backdrop-blur-sm border border-border/50 shadow-lg min-w-[200px]"
          >
            {isGeneratingSuggestions ? (
              // Loading state
              <div className="flex items-center justify-center py-6 gap-2">
                <Loader2 className="h-4 w-4 animate-spin text-purple-500" />
                <span className="text-xs text-muted-foreground">Analyzing story...</span>
              </div>
            ) : suggestions.length > 0 ? (
              // Show generated suggestions
              <>
                <div className="flex items-center gap-1 px-2 pb-1 border-b border-border/30">
                  <Sparkles className="h-3 w-3 text-purple-500" />
                  <span className="text-[10px] text-muted-foreground font-medium">AI Suggestions</span>
                </div>
                {suggestions.map((suggestion, index) => (
                  <motion.button
                    key={suggestion.id}
                    initial={{ opacity: 0, y: 5 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ delay: index * 0.05 }}
                    onClick={() => handleSelectSuggestion(suggestion)}
                    disabled={isGenerating}
                    className={cn(
                      "px-3 py-2 rounded-lg text-sm text-left transition-all",
                      "hover:bg-violet-500/20 hover:text-violet-600 dark:hover:text-violet-300",
                      "border border-transparent hover:border-violet-500/30",
                      "disabled:opacity-50 disabled:cursor-not-allowed"
                    )}
                  >
                    {suggestion.text}
                  </motion.button>
                ))}
              </>
            ) : (
              // No suggestions yet
              <div className="text-center py-4 text-muted-foreground text-xs">
                Click a tool to generate suggestions
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* Stop Button - Shows when generating */}
      <AnimatePresence>
        {isGenerating && (
          <motion.div
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
          >
            <Button
              variant="ghost"
              size="icon"
              onClick={handleStopGeneration}
              className="h-10 w-10 rounded-xl bg-red-500/20 hover:bg-red-500/30 text-red-500 border border-red-500/30"
              title="Stop"
            >
              <Square className="h-4 w-4" />
            </Button>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
