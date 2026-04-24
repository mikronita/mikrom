"use client";

import { useState, Suspense, type FormEvent } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import Link from "next/link";
import { Box, Loader2, AlertCircle, CheckCircle2 } from "lucide-react";

import { login } from "@/lib/api";
import { setToken } from "@/lib/auth";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Alert, AlertDescription } from "@/components/ui/alert";

function LoginForm() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  const showSuccess = searchParams.get("registered") === "true";

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError("");

    if (!email || !password) {
      setError("Email and password are required");
      return;
    }

    setIsLoading(true);
    const result = await login({ email, password });
    setIsLoading(false);

    if (result.error) {
      setError(result.error);
    } else if (result.data) {
      setToken(result.data.token);
      router.push("/");
    }
  };

  return (
    <Card className="w-full max-w-md shadow-2xl border-zinc-200/50 dark:border-zinc-800/50">
      <CardHeader className="space-y-1 text-center">
        <div className="flex justify-center mb-4">
          <div className="w-12 h-12 bg-zinc-900 dark:bg-zinc-50 rounded-2xl flex items-center justify-center shadow-lg">
            <Box className="w-6 h-6 text-white dark:text-zinc-900" />
          </div>
        </div>
        <CardTitle className="text-2xl font-bold tracking-tight">Welcome back</CardTitle>
        <CardDescription>
          Enter your credentials to access your dashboard
        </CardDescription>
      </CardHeader>
      
      <CardContent>
        <form onSubmit={handleSubmit} className="space-y-4">
          {showSuccess && (
            <Alert variant="default" className="bg-green-50 dark:bg-green-900/20 border-green-200 dark:border-green-800 text-green-800 dark:text-green-300">
              <CheckCircle2 className="h-4 w-4" />
              <AlertDescription>
                Account created! You can now sign in.
              </AlertDescription>
            </Alert>
          )}

          {error && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>
                {error}
              </AlertDescription>
            </Alert>
          )}

          <div className="space-y-2">
            <Label htmlFor="email">Email address</Label>
            <Input
              id="email"
              type="email"
              placeholder="name@example.com"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              disabled={isLoading}
              required
            />
          </div>

          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label htmlFor="password">Password</Label>
              <button type="button" className="text-xs text-muted-foreground hover:text-foreground transition-colors">
                Forgot password?
              </button>
            </div>
            <Input
              id="password"
              type="password"
              placeholder="••••••••"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={isLoading}
              required
            />
          </div>

          <div className="flex flex-col gap-4 pt-2">
            <Button type="submit" disabled={isLoading} className="w-full">
              {isLoading ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Signing in...
                </>
              ) : (
                "Sign In"
              )}
            </Button>
            <div className="text-center text-sm text-muted-foreground">
              Don&apos;t have an account?{" "}
              <Link href="/auth/register" className="font-semibold text-foreground hover:underline">
                Create one for free
              </Link>
            </div>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}

export default function LoginPage() {
  return (
    <div className="min-h-screen flex flex-col items-center justify-center bg-zinc-50 dark:bg-zinc-950 px-4 relative overflow-hidden">
      {/* Background blobs */}
      <div className="absolute top-0 left-0 w-full h-full -z-10 opacity-30 pointer-events-none">
        <div className="absolute -top-24 -left-24 w-96 h-96 bg-zinc-200 dark:bg-zinc-800 rounded-full blur-[100px]" />
        <div className="absolute -bottom-24 -right-24 w-96 h-96 bg-zinc-200 dark:bg-zinc-800 rounded-full blur-[100px]" />
      </div>

      <Suspense fallback={<Loader2 className="w-8 h-8 animate-spin text-zinc-400" />}>
        <LoginForm />
      </Suspense>
    </div>
  );
}
