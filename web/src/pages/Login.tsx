import { useState } from "react";
import { useSearchParams } from "react-router-dom";
import {
  Box,
  Card,
  Flex,
  Text,
  TextField,
  Button,
  Tabs,
} from "@radix-ui/themes";
import { PersonIcon, LockClosedIcon } from "@radix-ui/react-icons";
import { motion } from "framer-motion";
import { LaserBackground } from "../components/LaserBackground";
import { useAuth } from "../hooks/useAuth";
import { LOGO_URL } from "../assets";

export function Login() {
  const { login, register } = useAuth();
  const [searchParams] = useSearchParams();
  const [tab, setTab] = useState<"login" | "register">(
    searchParams.get("tab") === "register" ? "register" : "login"
  );
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [focused, setFocused] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  // Speed: 1x default, 3x on typing/focus, 9x on submit
  const speedMultiplier = submitting ? 9 : focused ? 3 : 1;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setLoading(true);
    setSubmitting(true);

    try {
      if (tab === "login") {
        await login(username, password);
      } else {
        await register(username, password);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Authentication failed");
      setSubmitting(false);
    } finally {
      setLoading(false);
    }
  };

  return (
    <Box
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <LaserBackground speedMultiplier={speedMultiplier} />
      <div style={{ position: "fixed", inset: 0, zIndex: 0, backdropFilter: "blur(7px)", background: "rgba(6,6,14,0)" }} />
      <motion.div
        initial={{ opacity: 0, scale: 0.95 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.3 }}
        style={{ position: "relative", zIndex: 1, width: "100%", maxWidth: 400, padding: 16 }}
      >
        <Card
          size="4"
          style={{
            backdropFilter: "blur(24px)",
            background: "var(--color-panel-translucent)",
          }}
        >
          <Flex direction="column" gap="4">
            <Flex direction="column" align="center" gap="2">
              <img src={LOGO_URL} alt="StreamX" width={144} height={144} />
              <Text size="6" weight="bold">
                StreamX
              </Text>
              <Text size="2" color="gray">
                Video Streaming Platform
              </Text>
            </Flex>

            <Tabs.Root
              value={tab}
              onValueChange={(v) => {
                setTab(v as "login" | "register");
                setError(null);
              }}
            >
              <Tabs.List>
                <Tabs.Trigger value="login">Sign In</Tabs.Trigger>
                <Tabs.Trigger value="register">Create Account</Tabs.Trigger>
              </Tabs.List>
            </Tabs.Root>

            <form onSubmit={handleSubmit}>
              <Flex direction="column" gap="3">
                <TextField.Root
                  placeholder="Username"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  onFocus={() => setFocused(true)}
                  onBlur={() => setFocused(false)}
                  required
                >
                  <TextField.Slot>
                    <PersonIcon />
                  </TextField.Slot>
                </TextField.Root>

                <TextField.Root
                  type="password"
                  placeholder="Password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  onFocus={() => setFocused(true)}
                  onBlur={() => setFocused(false)}
                  required
                >
                  <TextField.Slot>
                    <LockClosedIcon />
                  </TextField.Slot>
                </TextField.Root>

                {error && (
                  <Text size="2" color="red">
                    {error}
                  </Text>
                )}

                <Button type="submit" size="3" disabled={loading}>
                  {loading
                    ? "Please wait..."
                    : tab === "login"
                      ? "Sign In"
                      : "Create Account"}
                </Button>
              </Flex>
            </form>
          </Flex>
        </Card>
      </motion.div>
    </Box>
  );
}
