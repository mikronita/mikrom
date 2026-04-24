"use client";

import { useState, Suspense, type FormEvent } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import Link from "next/link";
import { Box, Loader2, AlertCircle, CheckCircle2 } from "lucide-react";

import { login } from "@/lib/api";
import { setToken } from "@/lib/auth";
import { Button, Card, Label, TextInput, Alert } from "flowbite-react";

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
    <Card className="w-full max-w-md shadow-2xl dark:bg-zinc-900 border-zinc-200/50 dark:border-zinc-800/50">
      <div className="space-y-1 text-center">
        <div className="flex justify-center mb-4">
          <div className="w-12 h-12 bg-zinc-900 dark:bg-zinc-50 rounded-2xl flex items-center justify-center shadow-lg">
            <Box className="w-6 h-6 text-white dark:text-zinc-900" />
          </div>
        </div>
        <h2 className="text-2xl font-bold tracking-tight dark:text-white">Welcome back</h2>
        <p className="text-sm text-zinc-500 dark:text-zinc-400">
          Enter your credentials to access your dashboard
        </p>
      </div>
      
      <form onSubmit={handleSubmit} className="space-y-4">
        {showSuccess && (
          <Alert color="success" icon={() => <CheckCircle2 className="w-4 h-4 mr-2" />}>
            Account created! You can now sign in.
          </Alert>
        )}

        {error && (
          <Alert color="failure" icon={() => <AlertCircle className="w-4 h-4 mr-2" />}>
            {error}
          </Alert>
        )}

        <div>
          <div className="mb-2 block">
            <Label htmlFor="email">Email address</Label>
          </div>
          <TextInput
            id="email"
            type="email"
            placeholder="name@example.com"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            disabled={isLoading}
            required
          />
        </div>

        <div>
          <div className="flex items-center justify-between mb-2">
            <Label htmlFor="password">Password</Label>
            <button type="button" className="text-xs text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-300 transition-colors">
              Forgot password?
            </button>
          </div>
          <TextInput
            id="password"
            type="password"
            placeholder="••••••••"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            disabled={isLoading}
            required
          />
        </div>

        <div className="flex flex-col gap-4">
          <Button type="submit" color="blue" disabled={isLoading} className="w-full">
            {isLoading ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                Signing in...
              </>
            ) : (
              "Sign In"
            )}
          </Button>
          <div className="text-center text-sm text-zinc-500">
            Don&apos;t have an account?{" "}
            <Link href="/auth/register" className="font-semibold text-zinc-900 dark:text-zinc-100 hover:underline">
              Create one for free
            </Link>
          </div>
        </div>
      </form>
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
